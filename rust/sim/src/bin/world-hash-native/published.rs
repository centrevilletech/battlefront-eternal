//! The third fingerprint: a SHA-1 over every message the local server sent
//! this tick, channel name then payload, in order (`tools/world-hash.mjs`:
//! `for (const [channel, payload] of sent) { published.update(channel);
//! published.update(payload) }`). `send` is called from
//! `src/runtime/local-server.js` `publish()`: the input acks as JSON on
//! `state`, then the edge results, diagnostics (always none) and gameplay
//! events as JSON on `events`, then the shared parts and own packets for the
//! client as bytes on `state`. The JSON texts are `JSON.stringify` of the
//! objects `src/runtime/wire-publish.js` builds, so their key order is that
//! file's.
use base64::Engine;
use bfe_sim::abi::context_step::Publication;
use bfe_sim::gen::{
    BlockHeader as B, WirePublicationOutput as P, WirePublishedAck as A, WirePublishedEdges as E,
    WirePublishedGroup as G, WirePublishedOutcome as O, WirePublishedOwn as Own,
    WirePublishedRecord as R, WirePublishedReliable as Rel, WireSpan as S,
};
use jsrt::numfmt::json_number;
use sha1::{Digest, Sha1};

/// The publication block's one row and its heap (`Publication::output`
/// decodes the same into a struct; this reader keeps the rows borrowed).
fn row_and_heap(p: &Publication) -> (&[f64], &[f64]) {
    let words = &p.block.words;
    let row = &words[B::WORDS..B::WORDS + P::WORDS];
    let heap_at = words[B::HEAP] as usize;
    let heap_len = words[B::HEAP_WORDS] as usize;
    (row, &words[heap_at..heap_at + heap_len])
}
fn table<'a>(row: &[f64], at: usize, stride: usize, heap: &'a [f64]) -> &'a [f64] {
    let start = row[at] as usize;
    let count = row[at + 1] as usize;
    &heap[start..start + count * stride]
}
fn span<'a>(bytes: &'a [u8], row: &[f64], at: usize) -> &'a [u8] {
    let offset = row[at + S::OFFSET] as usize;
    let length = row[at + S::LENGTH] as usize;
    &bytes[offset..offset + length]
}
/// `PublicationOperation.status`: the status text is UTF-16LE in the bytes.
fn utf16_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Every message `publish()` sent this tick, channel then payload bytes, in
/// order, to `sink`. `client` is the local client's pool key (the snapshot
/// host's native id 1 for the one local connection); `clients.get(id)` in
/// wire-publish.js names it "local", and a row for another client would
/// carry no `client` key at all (JSON.stringify drops undefined), which
/// cannot happen here.
pub fn each_sent(p: &Publication, client: u32, mut sink: impl FnMut(&str, &[u8])) {
    let (row, heap) = row_and_heap(p);
    let bytes = &p.bytes[..row[P::BYTE_LENGTH] as usize];
    let client_f = client as f64;
    let n = json_number;
    // inputAcks: {type:'input-ack', client, epoch, serverTick, generation,
    // ack, ackTick, dropped, discardedInput, discardedActions, ackActions: 0}
    for ack in table(row, P::ACKS, A::WORDS, heap).chunks_exact(A::WORDS) {
        let msg = format!(
            "{{\"type\":\"input-ack\"{},\"epoch\":{},\"serverTick\":{},\"generation\":{},\"ack\":{},\"ackTick\":{},\"dropped\":{},\"discardedInput\":{},\"discardedActions\":{},\"ackActions\":0}}",
            client_key(ack[A::CLIENT], client_f),
            n(row[P::EPOCH]),
            n(row[P::TICK]),
            n(ack[A::GENERATION]),
            n(ack[A::ACK]),
            n(ack[A::ACK_TICK]),
            n(row[P::DROPPED_TICKS]),
            n(ack[A::DISCARDED_INPUT]),
            n(ack[A::DISCARDED_ACTIONS])
        );
        sink("state", msg.as_bytes());
    }
    // edgeResults: {type:'entity-edge-results', client, epoch, generation,
    // received:[...], results:[{id,status,tick}]}
    for edge in table(row, P::EDGES, E::WORDS, heap).chunks_exact(E::WORDS) {
        let received: Vec<String> = table(edge, E::RECEIVED, 1, heap)
            .iter()
            .map(|v| n(*v))
            .collect();
        let results: Vec<String> = table(edge, E::RESULTS, O::WORDS, heap)
            .chunks_exact(O::WORDS)
            .map(|r| {
                format!(
                    "{{\"id\":{},\"status\":{},\"tick\":{}}}",
                    n(r[O::ID]),
                    serde_json::to_string(&utf16_text(span(bytes, r, O::STATUS)))
                        .unwrap_or_default(),
                    n(r[O::TICK])
                )
            })
            .collect();
        let msg = format!(
            "{{\"type\":\"entity-edge-results\"{},\"epoch\":{},\"generation\":{},\"received\":[{}],\"results\":[{}]}}",
            client_key(edge[E::CLIENT], client_f),
            n(edge[E::EPOCH]),
            n(edge[E::GENERATION]),
            received.join(","),
            results.join(",")
        );
        sink("events", msg.as_bytes());
    }
    // reliableEvents: {type:'gameplay-events', tick, records:[{kind:'combat',
    // id, generation, tick, schema, payload: base64}]}; the client key of
    // the row is not part of the message local-server.js builds.
    let records = table(row, P::RECORDS, R::WORDS, heap);
    for rel in table(row, P::RELIABLE, Rel::WORDS, heap).chunks_exact(Rel::WORDS) {
        let texts: Vec<String> = table(rel, Rel::RECORDS, 1, heap)
            .iter()
            .map(|index| {
                let r = &records[*index as usize * R::WORDS..(*index as usize + 1) * R::WORDS];
                format!(
                    "{{\"kind\":\"combat\",\"id\":{},\"generation\":{},\"tick\":{},\"schema\":{},\"payload\":\"{}\"}}",
                    n(r[R::ID]),
                    n(r[R::GENERATION]),
                    n(r[R::TICK]),
                    n(r[R::SCHEMA]),
                    base64::engine::general_purpose::STANDARD.encode(span(bytes, r, R::PAYLOAD))
                )
            })
            .collect();
        let msg = format!(
            "{{\"type\":\"gameplay-events\",\"tick\":{},\"records\":[{}]}}",
            n(row[P::TICK]),
            texts.join(",")
        );
        sink("events", msg.as_bytes());
    }
    // publicationBuffersFor(out, id): every shared group naming the client,
    // its parts in order, then the client's own packets.
    for group in table(row, P::SHARED, G::WORDS, heap).chunks_exact(G::WORDS) {
        if !table(group, G::CLIENTS, 1, heap).contains(&client_f) {
            continue;
        }
        for part in table(group, G::PARTS, S::WORDS, heap).chunks_exact(S::WORDS) {
            sink("state", span(bytes, part, 0));
        }
    }
    for own in table(row, P::OWN, Own::WORDS, heap).chunks_exact(Own::WORDS) {
        if own[Own::CLIENT] == client_f {
            sink("state", span(bytes, own, Own::SPAN));
        }
    }
}

/// One tick's `published` column: 16 hex characters of the SHA-1 over the
/// sent messages, channel name then payload each.
pub fn hash_published(p: &Publication, client: u32) -> String {
    let mut sha = Sha1::new();
    each_sent(p, client, |channel, payload| {
        sha.update(channel.as_bytes());
        sha.update(payload);
    });
    hex16(&sha.finalize())
}

/// The sent messages as text lines (`channel hex`), for a diff against the
/// Node tool's list.
pub fn sent_lines(p: &Publication, client: u32) -> String {
    let mut out = String::new();
    each_sent(p, client, |channel, payload| {
        out.push_str(channel);
        out.push(' ');
        for b in payload {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    });
    out
}

/// `,"client":"local"` for the local client, nothing for another.
fn client_key(row_client: f64, client: f64) -> &'static str {
    if row_client == client {
        ",\"client\":\"local\""
    } else {
        ""
    }
}

/// The first 16 hex characters of a digest (`hash()` in world-hash.mjs).
pub fn hex16(digest: &[u8]) -> String {
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `hash(text)`: 16 hex characters of SHA-1 over UTF-8 bytes.
pub fn sha1_hex16(text: &str) -> String {
    hex16(&Sha1::digest(text.as_bytes()))
}
