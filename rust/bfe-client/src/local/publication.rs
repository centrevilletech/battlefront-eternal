#![allow(clippy::chunks_exact_to_as_chunks)]
//! local-server.js::publish, using the world's retained publication directly.
//! Shared and own spans enter the existing Rust session without a socket,
//! transferable buffer or JSON envelope. Reliable cosmetic fields enter its
//! existing journal before datagrams, preserving the local server's order.
use bfe_client::online::receiver::Receiver;
use bfe_presentation::network_layout::{net_header as h, net_kind as k, net_record as r};
use bfe_sim::{
    gen::{
        BlockHeader as B, WirePublicationOutput as P, WirePublishedAck as A,
        WirePublishedEdges as E, WirePublishedFault as F, WirePublishedGroup as G,
        WirePublishedOutcome as Outcome, WirePublishedOwn as O, WirePublishedRecord as R,
        WirePublishedReliable as Reliable, WireSpan as S,
    },
    store::block::Block,
};

const OWN_MAGIC: u32 = 0x314f_4e42;

pub struct Publication<'a> {
    words: &'a [f64],
    row: &'a [f64],
    heap: &'a [f64],
    bytes: &'a [u8],
}

// Counts are schema integers, not saturating float casts. Even a corrupt
// native producer must fail at this boundary instead of slicing beyond its
// retained block. Normal publications take only these checked borrowed views.
fn integer(value: f64, maximum: usize) -> Result<usize, &'static str> {
    if !value.is_finite() || value < 0. || value.fract() != 0. || value > maximum as f64 {
        Err("invalid local publication count")
    } else {
        Ok(value as usize)
    }
}
fn rows<'a>(
    row: &[f64],
    at: usize,
    stride: usize,
    heap: &'a [f64],
) -> Result<&'a [f64], &'static str> {
    let start = integer(
        *row.get(at).ok_or("short local publication row")?,
        heap.len(),
    )?;
    let count = integer(
        *row.get(at + 1).ok_or("short local publication row")?,
        heap.len() / stride,
    )?;
    let end = count
        .checked_mul(stride)
        .and_then(|n| start.checked_add(n))
        .ok_or("local publication range overflow")?;
    heap.get(start..end)
        .ok_or("local publication range exceeds its heap")
}

impl<'a> Publication<'a> {
    pub fn new(block: &'a Block, bytes: &'a [u8]) -> Result<Self, &'static str> {
        // Context copies the generated encoder's block into retained storage.
        // Header offsets address that block; nested table offsets address only
        // its heap, while packet spans address the separate byte publication.
        let words = &block.words;
        if words.len() < B::WORDS
            || integer(words[B::ROWS], 1)? != 1
            || integer(words[B::STRIDE], P::WORDS)? != P::WORDS
        {
            return Err("invalid local publication block");
        }
        let start = integer(words[B::HEAP], words.len())?;
        let count = integer(words[B::HEAP_WORDS], words.len())?;
        let end = start
            .checked_add(count)
            .ok_or("local publication heap overflow")?;
        let row = words
            .get(B::WORDS..B::WORDS + P::WORDS)
            .ok_or("short local publication header")?;
        if start < B::WORDS + P::WORDS {
            return Err("local publication heap overlaps its row");
        }
        let heap = words
            .get(start..end)
            .ok_or("short local publication heap")?;
        let length = integer(row[P::BYTE_LENGTH], bytes.len())?;
        Ok(Self {
            words: &words[..end],
            row,
            heap,
            bytes: &bytes[..length],
        })
    }

    /// Packs the admission's exact descriptor using its generated layout. The
    /// spans still refer to the original byte arena, including reliable records.
    pub fn from_welcome(
        output: &P,
        bytes: &'a [u8],
        block: &'a mut Block,
    ) -> Result<Self, &'static str> {
        block.words.clear();
        block.words.resize(B::WORDS + P::WORDS, 0.0);
        block.heap.clear();
        output.put(
            &mut block.words[B::WORDS..B::WORDS + P::WORDS],
            &mut block.heap,
        );
        let heap_words = block.heap.len();
        block.words[B::ROWS] = 1.0;
        block.words[B::STRIDE] = P::WORDS as f64;
        block.words[B::HEAP] = (B::WORDS + P::WORDS) as f64;
        block.words[B::HEAP_WORDS] = heap_words as f64;
        block.words.append(&mut block.heap);
        Self::new(block, bytes)
    }

    /// Copies the validated publication block and byte span into a pre-allocated
    /// slot for cross-thread delivery without heap allocation.
    pub fn copy_into(&self, block: &mut Block, out_bytes: &mut Vec<u8>) {
        block.words.clear();
        block.words.extend_from_slice(self.words);
        out_bytes.clear();
        out_bytes.extend_from_slice(self.bytes);
    }

    /// Counts combat and presentation records published for `client` in this tick
    /// across both reliable records (`P::RELIABLE`) and cosmetic records packed into
    /// `OWN` datagrams (`P::OWN` section `flags & 4 != 0`) without allocating on the heap.
    /// Returns `(presentation_schema_8_records, total_records)`.
    pub fn count_client_records(&self, client: u32) -> (u64, u64) {
        let mut presentation = 0u64;
        let mut total = 0u64;
        if let (Ok(records), Ok(reliables)) = (
            self.table(P::RECORDS, R::WORDS),
            self.table(P::RELIABLE, Reliable::WORDS),
        ) {
            let max_records = records.len() / R::WORDS;
            for reliable in reliables.chunks_exact(Reliable::WORDS) {
                if reliable[Reliable::CLIENT] != client as f64 {
                    continue;
                }
                let Ok(indices) = rows(reliable, Reliable::RECORDS, 1, self.heap) else {
                    continue;
                };
                for index in indices {
                    let Ok(idx) = integer(*index, max_records) else {
                        continue;
                    };
                    if let Some(record) = records.get(idx * R::WORDS..(idx + 1) * R::WORDS) {
                        total += 1;
                        if record[R::SCHEMA] == 8.0 {
                            presentation += 1;
                        }
                    }
                }
            }
        }
        // Cosmetic presentation records (`PRESENTATION_SCHEMA = 8`) and bolt/explosion
        // records (`schema = 1, 2, 5, 6, 7`) are packed into `OWN` datagrams under
        // section bit 4 (`cosmetics`). Each entry in that section is `[u16 len][bytes]`
        // where `bytes[12..14]` is the little-endian `u16` record schema.
        if let Ok(owns) = self.table(P::OWN, O::WORDS) {
            for own in owns.chunks_exact(O::WORDS) {
                if own[O::CLIENT] != client as f64 {
                    continue;
                }
                let Ok(bytes) = self.span(own, O::SPAN) else {
                    continue;
                };
                if bytes.len() < 21 {
                    continue;
                }
                let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                if magic != OWN_MAGIC {
                    continue;
                }
                let flags = bytes[20];
                let mut at = 21usize;
                for bit in [1u8, 2u8] {
                    if flags & bit != 0 {
                        let Some(len_bytes) = bytes.get(at..at + 2) else {
                            at = bytes.len() + 1;
                            break;
                        };
                        let section_len =
                            u16::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
                        at = at.saturating_add(2 + section_len);
                    }
                }
                if flags & 4 != 0 {
                    let Some(len_bytes) = bytes.get(at..at + 2) else {
                        continue;
                    };
                    let section_len = u16::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
                    at += 2;
                    let end = (at + section_len).min(bytes.len());
                    while at + 2 <= end {
                        let ev_len =
                            u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
                        at += 2;
                        if at + ev_len > end {
                            break;
                        }
                        if ev_len >= 14 {
                            let schema =
                                u16::from_le_bytes(bytes[at + 12..at + 14].try_into().unwrap());
                            total += 1;
                            if schema == 8 {
                                presentation += 1;
                            }
                        }
                        at += ev_len;
                    }
                }
            }
        }
        (presentation, total)
    }

    fn table(&self, at: usize, stride: usize) -> Result<&'a [f64], &'static str> {
        rows(self.row, at, stride, self.heap)
    }
    fn span(&self, row: &[f64], at: usize) -> Result<&'a [u8], &'static str> {
        let offset = integer(
            *row.get(at + S::OFFSET)
                .ok_or("short local publication span")?,
            self.bytes.len(),
        )?;
        let length = integer(
            *row.get(at + S::LENGTH)
                .ok_or("short local publication span")?,
            self.bytes.len(),
        )?;
        let end = offset
            .checked_add(length)
            .ok_or("local publication byte range overflow")?;
        self.bytes
            .get(offset..end)
            .ok_or("local publication bytes exceed their owner")
    }

    // A publication may contain other consumers. Select the admitted local
    // client by its pool key, not by group position or an assumed first row.
    // Faults for client zero apply to the whole world, as SnapshotHost does.
    pub fn deliver(&self, client: u32, receiver: &mut Receiver, now_ms: f64) -> Result<(), String> {
        for fault in self.table(P::FAULTS, F::WORDS)?.chunks_exact(F::WORDS) {
            if fault[F::CLIENT] == 0. || fault[F::CLIENT] == client as f64 {
                return Err(String::from_utf8_lossy(self.span(fault, F::MESSAGE)?)
                    .chars()
                    .take(240)
                    .collect());
            }
        }
        for ack in self.table(P::ACKS, A::WORDS)?.chunks_exact(A::WORDS) {
            // A respawn can share an epoch but never the previous generation's
            // input acknowledgement. Pair both before advancing its receipt.
            if ack[A::CLIENT] == client as f64 && ack[A::GENERATION] == receiver.own.generation {
                receiver.ack = receiver.ack.max(ack[A::ACK]);
            }
        }
        for edge in self.table(P::EDGES, E::WORDS)?.chunks_exact(E::WORDS) {
            // These bounded receipt sets are handed back after the complete
            // publication. Local transport has no later packet to repeat them.
            if edge[E::CLIENT] != client as f64
                || edge[E::EPOCH] != receiver.epoch
                || edge[E::GENERATION] != receiver.own.generation
            {
                continue;
            }
            for id in rows(edge, E::RECEIVED, 1, self.heap)? {
                receiver
                    .received
                    .add(integer(*id, u32::MAX as usize)? as u32);
            }
            for result in
                rows(edge, E::RESULTS, Outcome::WORDS, self.heap)?.chunks_exact(Outcome::WORDS)
            {
                receiver
                    .outcomes
                    .add(integer(result[Outcome::ID], u32::MAX as usize)? as u32);
            }
        }
        // createWirePublisher's reliable lists index its one record table.
        // Copy the payload once into the existing bounded journal, whose
        // identity window also suppresses its later datagram representation.
        let records = self.table(P::RECORDS, R::WORDS)?;
        for reliable in self
            .table(P::RELIABLE, Reliable::WORDS)?
            .chunks_exact(Reliable::WORDS)
        {
            if reliable[Reliable::CLIENT] != client as f64 {
                continue;
            }
            for index in rows(reliable, Reliable::RECORDS, 1, self.heap)? {
                let index = integer(*index, records.len() / R::WORDS)?;
                let record = records
                    .get(index * R::WORDS..(index + 1) * R::WORDS)
                    .ok_or("missing local reliable record")?;
                let mut row = [0.; r::STRIDE];
                row[r::KIND] = k::COMBAT as f64;
                row[r::ID] = record[R::ID];
                row[r::GENERATION] = record[R::GENERATION];
                row[r::TICK] = record[R::TICK];
                row[r::SCHEMA] = record[R::SCHEMA];
                receiver
                    .publications
                    .push(&row, self.span(record, R::PAYLOAD)?);
            }
        }
        for group in self.table(P::SHARED, G::WORDS)?.chunks_exact(G::WORDS) {
            // Shared baselines can serve many admitted consumers. Borrow each
            // selected span once; the session already owns baseline decoding.
            if !rows(group, G::CLIENTS, 1, self.heap)?.contains(&(client as f64)) {
                continue;
            }
            for part in rows(group, G::PARTS, S::WORDS, self.heap)?.chunks_exact(S::WORDS) {
                receiver.message(false, self.span(part, 0)?, now_ms);
            }
        }
        for own in self.table(P::OWN, O::WORDS)?.chunks_exact(O::WORDS) {
            // Own checkpoints follow shared records, as in local-server.js.
            // Presentation's replay owner resolves the resulting control lease.
            if own[O::CLIENT] == client as f64 {
                receiver.message(false, self.span(own, O::SPAN)?, now_ms);
            }
        }
        receiver.error().map_or(Ok(()), Err)
    }

    // Cumulative receipt facts return to the same publisher after delivery.
    // The world's own sample ring remains canonical; these scalars neither
    // reconstruct entities nor introduce a second acknowledgement algorithm.
    pub fn acknowledgement(receiver: &Receiver) -> [f64; 2] {
        let owner = receiver.network.borrow();
        [owner.header[h::EPOCH], owner.header[h::ACK_TICK]]
    }
}
