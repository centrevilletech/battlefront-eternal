//! Whole-world boundaries used by WorldTick and createWirePublisher. Numeric
//! input borrows end before output writes or V8 buffer creation. Context owns
//! each reply; no packet ever borrows the reusable native encoder storage.
use super::*;
use super::operations::{invalid, Arenas, Request};
use bfe_sim::abi::context::{Error as ContextError, ErrorKind};
use bfe_sim::abi::context_step::Publication;
use bfe_sim::gen::*;
use napi::JsObject;

#[napi(object)]
pub struct PublicationFacts {
    pub session: u32,
    pub words: JsObject,
}
#[napi(object)]
pub struct Published {
    pub words: u32,
    pub packets: JsObject,
}

// Each location is a generated WireSpan in the numeric block. Sorting by its
// byte range lets repeated references share a single owned ArrayBuffer without
// a per-publication hash table or an extra intermediate copy of packet bytes.
#[derive(Clone, Copy, Debug)]
struct Span { word: usize, offset: usize, length: usize }
#[derive(Default)]
pub(super) struct Packets {
    bytes: Vec<u8>,
    spans: Vec<Span>,
    prepared: bool,
}
impl Packets {
    pub fn reserve(&mut self, words: usize, bytes: usize) -> Result<()> {
        self.bytes.try_reserve_exact(bytes.saturating_sub(self.bytes.len())).map_err(|_| invalid("Publication byte capacity"))?;
        // No block can contain more distinct two-word spans than half its
        // words. Reserve their locations alongside the numeric admission arena.
        self.spans.try_reserve_exact((words / WireSpan::WORDS).saturating_sub(self.spans.len()))
            .map_err(|_| invalid("Packet span capacity"))
    }
    fn clear(&mut self) { self.bytes.clear(); self.spans.clear(); self.prepared = false; }
    pub fn memory_bytes(&self) -> (usize, usize) {
        (self.bytes.capacity() + self.spans.capacity() * std::mem::size_of::<Span>(),
         self.bytes.len() + self.spans.len() * std::mem::size_of::<Span>())
    }
    fn collect(&mut self, block: &[f64]) -> Result<()> {
        self.prepared = false;
        super::blocks::validate(block)?;
        if block[BlockHeader::ROWS] != 1.0 || block[BlockHeader::STRIDE] != WirePublicationOutput::WORDS as f64 {
            return Err(invalid("Invalid publication block"));
        }
        let start = BlockHeader::WORDS;
        let heap_at = block[BlockHeader::HEAP] as usize;
        let row = &block[start..heap_at];
        let heap = &block[heap_at..];
        self.spans.clear();
        let mut add = |row: &[f64], field: usize| -> Result<()> {
            if self.spans.len() == self.spans.capacity() {return Err(invalid("Packet spans require admission capacity"));}
            let span = WireSpan::take(&row[field..], heap);
            let offset = span.offset as usize;
            let length = span.length as usize;
            if offset.checked_add(length).is_none_or(|end| end > self.bytes.len()) {
                return Err(invalid("Invalid publication byte span"));
            }
            // Both slices belong to this validated block; only this numeric
            // index is retained, never a pointer into the block or V8 memory.
            let word = (row.as_ptr() as usize - block.as_ptr() as usize) / std::mem::size_of::<f64>() + field;
            self.spans.push(Span {word, offset, length});
            Ok(())
        };
        for group in WirePublicationOutput::shared_rows(row, heap) {
            for part in WirePublishedGroup::parts_rows(group, heap) { add(part, 0)?; }
        }
        for own in WirePublicationOutput::own_rows(row, heap) { add(own, WirePublishedOwn::SPAN)?; }
        for edges in WirePublicationOutput::edges_rows(row, heap) {
            for outcome in WirePublishedEdges::results_rows(edges, heap) { add(outcome, WirePublishedOutcome::STATUS)?; }
        }
        for record in WirePublicationOutput::records_rows(row, heap) { add(record, WirePublishedRecord::PAYLOAD)?; }
        for fault in WirePublicationOutput::faults_rows(row, heap) { add(fault, WirePublishedFault::MESSAGE)?; }
        self.spans.sort_unstable_by_key(|span| (span.offset, span.length));
        self.prepared = true;
        Ok(())
    }
    fn deliver(&mut self, env: Env, arenas: &mut Arenas, output: &blocks::Arena) -> Result<Published> {
        // Capacity leaves the retained native reply and all destination words
        // untouched. No buffers are handed out until the whole reply fits.
        let packets = env.create_array_with_length(0)?;
        if output.length < arenas.block.words.len() {
            arenas.last_fault = Some("Capacity");
            return Ok(Published {words: arenas.block.words.len() as u32, packets});
        }
        if !self.prepared { self.collect(&arenas.block.words)?; }
        let mut packets = packets;
        let mut previous = None;
        let mut count = 0u32;
        for span in &self.spans {
            let key = (span.offset, span.length);
            if previous != Some(key) {
                let mut buffer = env.create_arraybuffer(span.length)?;
                if span.length != 0 {
                    buffer.as_mut().copy_from_slice(&self.bytes[span.offset..span.offset + span.length]);
                }
                packets.set_element(count, buffer.value)?;
                previous = Some(key); count += 1;
            }
            // A span's offset now addresses packets[], preserving its length.
            // Generated readers need no pointer or backing-store view to copy it.
            arenas.block.words[span.word + WireSpan::OFFSET] = (count - 1) as f64;
        }
        let words = arenas.deliver(output);
        self.clear();
        Ok(Published {words, packets})
    }
}


// Admission owns growth. The byte window is independent of numeric metadata:
// a small result may refer to many packet bytes, so never size one by the other.
pub(super) fn reserve_owner(owner: &mut OwnedContext, reserve: Option<super::operations::Reserve>) -> Result<()> {
    let bytes = reserve.as_ref().and_then(|reserve| reserve.publication_bytes).unwrap_or(64 * 1024) as usize;
    if bytes > 64 * 1024 * 1024 {return Err(invalid("Publication reservation exceeds admission capacity"));}
    owner.arenas.reserve(reserve)?;
    owner.publication.reserve(owner.arenas.block.words.capacity(), bytes)
}

// Context catches native world panics itself. Turn its first Poisoned reply
// into GenericFailure too, and latch the addon guard before the next call.
pub(super) fn context_result<T>(owner: &mut OwnedContext, result: std::result::Result<T, ContextError>) -> Result<T> {
    result.map_err(|error| {
        if error.kind() == ErrorKind::Poisoned {
            owner.poisoned = true;
            owner.arenas.last_fault = Some("Poisoned");
            Error::new(Status::GenericFailure, "Native host panic: context poisoned")
        } else { Error::from_reason(format!("{:?}: {}", error.kind(), error.message())) }
    })
}

impl NativeOwner {
    pub(super) fn step_with(&self, env: Env, input: JsObject, out: JsObject, before: impl FnOnce()) -> Result<u32> {
        self.invoke(|owner| {
            let input = blocks::arena(env, &input)?;
            let output = blocks::arena(env, &out)?;
            if input.length != WorldTickInput::WORDS {return Err(invalid("Invalid world tick row"));}
            input.read(|words| {
                if !owner.arenas.matches(Request::Step, words)? {
                    before();
                    let result = owner.context.as_ref().unwrap().world_step(WorldTickInput::take(words, &[]));
                    let reply = context_result(owner, result)?;
                    // Admission reserves at least one fixed input/output row.
                    // Retaining these rows allocates nothing, even if out is
                    // short. Retrying copies the reply without stepping again.
                    let mut row = [0.; WorldTickOutput::WORDS];
                    reply.put(&mut row, &mut Vec::new());
                    owner.arenas.retain(Request::Step, words, &row);
                }
                Ok(())
            })?;
            Ok(owner.arenas.deliver(&output))
        })
    }
}

#[napi]
impl NativeOwner {
    // This is an explicit admission boundary, including delivery of a retained
    // short reply. Ordinary step calls never increase the numeric arenas.
    #[napi]
    pub fn reserve(&self, reserve: super::operations::Reserve) -> Result<()> {
        self.invoke(|owner| {
            reserve_owner(owner, Some(reserve))
        })
    }
    #[napi]
    pub fn step_init(&self, env: Env, setup: JsObject) -> Result<()> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            let input = blocks::arena(env, &setup)?;
            if input.length != WorldTickSetup::WORDS {return Err(invalid("Invalid world setup row"));}
            input.read(|words| {
                let result = owner.context.as_ref().unwrap().world_step_init(WorldTickSetup::take(words, &[]));
                context_result(owner, result)
            })
        })
    }
    #[napi]
    pub fn step(&self, env: Env, input: JsObject, out: JsObject) -> Result<u32> {
        self.step_with(env, input, out, || {})
    }
    #[napi]
    pub fn read_bank(&self, env: Env, bank: u32, out: JsObject) -> Result<u32> {
        self.invoke(|owner| {
            let output = blocks::arena(env, &out)?;
            let request = Request::Bank(bank);
            if !owner.arenas.matches(request, &[])? {
                let result = owner.context.as_ref().unwrap().read_bank(bank, &mut owner.arenas.block.words);
                let length = context_result(owner, result)?;
                if length != owner.arenas.block.words.len() || length > u32::MAX as usize {
                    return Err(invalid("Invalid bank extent"));
                }
                owner.arenas.args.clear(); owner.arenas.pending = Some(request);
            }
            Ok(owner.arenas.deliver(&output))
        })
    }
    #[napi]
    pub fn publish(&self, env: Env, facts: PublicationFacts, out: JsObject) -> Result<Published> {
        self.invoke(|owner| {
            let input = blocks::arena(env, &facts.words)?;
            let output = blocks::arena(env, &out)?;
            let request = Request::Publish(facts.session);
            input.read(|words| {
                if words.len() < WireWorldFacts::WORDS {return Err(invalid("Invalid publication facts row"));}
                // Retrying a publication retains its input bits. Reject an
                // unadmitted heap before Context can publish or mutate it.
                owner.arenas.input_capacity(words)?;
                if !owner.arenas.matches(request, words)? {
                    // Lend the reserved buffers to Context without cloning or
                    // reallocating a boundary reply. Restore the buffers before
                    // propagating even a poisoned/unavailable Context error.
                    let mut reply = Publication {block: std::mem::take(&mut owner.arenas.block),
                        bytes: std::mem::take(&mut owner.publication.bytes)};
                    let result = owner.context.as_ref().unwrap().world_publish_into(facts.session, words, &mut reply);
                    owner.arenas.block = reply.block; owner.publication.bytes = reply.bytes;
                    context_result(owner, result)?;
                    owner.arenas.args.clear(); owner.arenas.args.extend_from_slice(words);
                    owner.arenas.pending = Some(request);
                    owner.publication.prepared = false;
                }
                Ok(())
            })?;
            owner.publication.deliver(env, &mut owner.arenas, &output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_publication_spans_are_bounded_and_deduplicated_without_payload_copies() {
        let mut out = WirePublicationOutput::default();
        out.shared.push(WirePublishedGroup {parts: vec![WireSpan {offset: 0, length: 3}], ..Default::default()});
        out.own.push(WirePublishedOwn {span: WireSpan {offset: 0, length: 3}, ..Default::default()});
        out.records.push(WirePublishedRecord {payload: WireSpan {offset: 3, length: 1}, ..Default::default()});
        let mut block = Block::default(); block.begin(1, WirePublicationOutput::WORDS);
        let (row, heap) = block.row(0); out.put(row, heap); block.finish();
        let mut packets = Packets::default(); packets.reserve(block.words.len(), 4).unwrap();
        packets.bytes = vec![1, 2, 3, 4]; packets.collect(&block.words).unwrap();
        assert_eq!(packets.spans.len(), 3);
        assert_eq!((packets.spans[0].offset, packets.spans[1].offset, packets.spans[2].offset), (0, 0, 3));
        packets.bytes.truncate(3); assert!(packets.collect(&block.words).is_err());
    }
}

// Synthetic wire records exercise the production packet delivery code without
// pretending that a Context has a world. No production method consults this
// fixture, and readiness always comes from the simulation's capabilities.
#[cfg(feature = "lifecycle-probe")]
#[napi]
impl NativeOwner {
    #[napi]
    pub fn probe_packet_buffers(&self, env: Env, out: JsObject, bad_span: bool) -> Result<Published> {
        self.invoke(|_| {
            let output = blocks::arena(env, &out)?;
            let mut record = WirePublicationOutput {epoch: -0., tick: f64::from_bits(0x7ff8_0000_0000_0042), byte_length: 7, ..Default::default()};
            record.shared.push(WirePublishedGroup {parts: vec![WireSpan {offset: 0, length: 3}, WireSpan {offset: 3, length: 2}], ..Default::default()});
            record.own.push(WirePublishedOwn {span: WireSpan {offset: 0, length: 3}, ..Default::default()});
            record.edges.push(WirePublishedEdges {results: vec![WirePublishedOutcome {status: WireSpan {offset: 5, length: 1}, ..Default::default()}], ..Default::default()});
            record.records.push(WirePublishedRecord {payload: WireSpan {offset: 3, length: 2}, ..Default::default()});
            record.faults.push(WirePublishedFault {message: WireSpan {offset: 6, length: if bad_span {2} else {1}}, ..Default::default()});
            let mut block = Block::default(); block.begin(1, WirePublicationOutput::WORDS);
            let (row, heap) = block.row(0); record.put(row, heap); block.finish();
            let mut arenas = Arenas::default(); arenas.reserve(None)?;
            arenas.retain(Request::Publish(0), &[], &block.words);
            // A short destination deliberately admits no span locations. The
            // capacity reply must precede span collection or V8 allocation.
            let mut packets = Packets::default(); packets.reserve(output.length, 7)?;
            packets.bytes = vec![0, 1, 255, 3, 4, 5, 6];
            packets.deliver(env, &mut arenas, &output)
        })
    }
}
