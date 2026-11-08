//! Native UDP interoperability test suite with two native endpoints.
//!
//! Mirrors `tests/native-transport.mjs`:
//! - Handshake authentication, stateless retry tokens, and duplicate response confirmation
//! - All three message lanes (`State`, `Events`, `Bulk`) with binary and text payloads
//! - Head-of-line isolation across independent ordered streams and the replaceable state lane
//! - Reliable stream convergence under deterministic bidirectional packet loss
//! - AEAD authentication failure and replay-window protection without advancing valid packets
//! - Key-phase rotation in both directions while ordered stream delivery remains exact
//! - Authenticated malformed ACK rejection (`close_code::PROTOCOL`), ordinary close propagation,
//!   oversized frame bounds refusal, and wrong server identity rejection (`Event::IdentityMismatch`)
//! - Idle keepalive exchange and authenticated idle timeout (`close_code::IDLE`)
//! - Live UDP socket `Endpoint` and `ServerEndpoint` authentication, lane exchange, and reconnect
//!   with one persistent Ed25519 identity
//!
//! All tests run in-process between two native Rust endpoints (`ClientHandshake` / `Connection` /
//! `Endpoint` and `ServerHandshakes` / `Connection` / `ServerEndpoint`) without Node or N-API.
//!
//! Why this test lives in `rust/bfe-transport/tests/interop.rs`:
//! - Replaces the Node-hosted `tests/native-transport.mjs` harness so `cargo test -p bfe-transport`
//!   exercises full two-endpoint interoperability in CI without a JavaScript runtime or N-API addon.
//! - Uses deterministic synthetic clocks (`now` in milliseconds) for protocol state-machine tests
//!   so loss recovery, selective ACKs, key rotation, and idle timeouts execute in milliseconds
//!   with zero timing flake.
//! - Uses OS-assigned loopback UDP sockets (`127.0.0.1:0`) for the live socket test to prove
//!   `Endpoint` and `ServerEndpoint` frame, encrypt, and reassemble real kernel UDP datagrams.

#![deny(warnings)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use bfe_transport::codec::Writer;
use bfe_transport::connection::{Closed, Connection, Lane, Sink};
use bfe_transport::crypto::{Identity, RetryTokens};
use bfe_transport::endpoint::Endpoint as ClientEndpoint;
use bfe_transport::handshake::{ClientHandshake, Event, ServerHandshakes};
use bfe_transport::layout::{
    close_code, data as d, frame as f, limits as lim, network_contract as nc, packet as p,
};
use bfe_transport::server::{AdmitDecision, ClientInfo, ServerEndpoint, ServerSink};

// Deterministic Ed25519 and HMAC seeds matching `seed = Buffer.alloc(32, 9)`,
// `identity(17)`, and `RetryTokens(Buffer.alloc(32, 31))` in `tests/native-transport.mjs`.
// Keeping the exact byte seeds ensures identity hex strings and derived transcripts
// behave identically to the JavaScript interoperability harness.
// `CLIENT_SEED` produces client identity `identity(9)`.
// `SERVER_SEED` produces server identity `identity(17)`.
// `RETRY_SECRET` seeds the server's stateless retry token HMAC key (`RetryTokens`).
const CLIENT_SEED: [u8; 32] = [9u8; 32];
const SERVER_SEED: [u8; 32] = [17u8; 32];
const RETRY_SECRET: [u8; 32] = [31u8; 32];

// Recorded application message delivered to an endpoint's Sink callback.
// Mirrors `Delivery` in `rust/bfe-transport/src/node.rs` and `messages` in `tests/native-transport.mjs`.
// Each delivery records the transport lane (`State`, `Events`, or `Bulk`), whether the wire frame
// marked the payload as UTF-8 text (`text == true`), and a copy of the reassembled payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DeliveredMessage {
    lane: Lane,
    text: bool,
    bytes: Vec<u8>,
}

// Explicit test observer capturing outgoing wire datagrams and incoming application messages.
// Mirrors `Observations` in `rust/bfe-transport/src/node.rs`.
// In production, `Connection` borrows a stack sink without allocating; in this test suite,
// copying packets into `packets` lets each test inspect, drop, reorder, or corrupt wire bytes.
#[derive(Default)]
struct EndpointObserver {
    packets: Vec<(Vec<u8>, SocketAddr)>,
    messages: Vec<DeliveredMessage>,
}

impl Sink for EndpointObserver {
    fn packet(&mut self, bytes: &[u8], path: SocketAddr) {
        // Every emitted UDP datagram must respect the 1,200-byte protocol ceiling
        // (`assert.ok(bytes.length <= MAX_DATAGRAM)` in `tests/native-transport.mjs`).
        // Enforcing the bound here verifies that neither datagram fragmentation nor stream
        // coalescing ever overflows a single UDP packet on the wire.
        assert!(
            bytes.len() <= lim::MAX_DATAGRAM,
            "emitted datagram {} exceeds MAX_DATAGRAM {}",
            bytes.len(),
            lim::MAX_DATAGRAM
        );
        self.packets.push((bytes.to_vec(), path));
    }

    fn message(&mut self, lane: Lane, text: bool, bytes: &[u8]) {
        // Copy borrowed delivery slice into the test observer's message log so assertions
        // can inspect the sequence of received payloads after `transfer` and `advance`.
        self.messages.push(DeliveredMessage {
            lane,
            text,
            bytes: bytes.to_vec(),
        });
    }
}

// Builds the synthetic application hello object matching `hello` in `tests/native-transport.mjs`:
// `{protocol: NETWORK_CONTRACT.protocol, profile: NETWORK_CONTRACT.profile, simulation: NETWORK_CONTRACT.simulation, content: 'synthetic-content', name: 'Player'}`.
// Both `ClientHandshake` and `ServerHandshakes` sign these exact serialized JSON bytes into the
// handshake transcript before deriving session traffic keys.
fn hello_value() -> serde_json::Value {
    serde_json::json!({
        "protocol": nc::PROTOCOL,
        "profile": nc::PROFILE,
        "simulation": nc::SIMULATION,
        "content": "synthetic-content",
        "name": "Player"
    })
}

// Synthetic loopback client address used for in-memory packet exchanges.
// Matches the client socket address passed to `server.receive(bytes, client_addr(), ...)`.
fn client_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49001)
}

// Synthetic loopback server address matching `address()` in `rust/bfe-transport/src/node.rs`.
// Matches the server socket address passed to `client.receive(bytes, server_addr(), ...)`.
fn server_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18451)
}

/// Paired native client and native server connections established through the
/// full `ClientHandshake` and `ServerHandshakes` state machines.
/// Mirrors `pair({ keyInterval, retry })` in `tests/native-transport.mjs`.
struct NativePair {
    client: Connection,
    server: Connection,
    client_obs: EndpointObserver,
    server_obs: EndpointObserver,
    now: f64,
}

impl NativePair {
    // Runs the 3-packet BFE-UDP/1 handshake (with optional stateless RETRY cookie exchange)
    // and transitions both peers into established `Connection` instances.
    // Verifies mutual Ed25519 identity proofs, server hello JSON preservation, duplicate
    // RESPONSE handling, and initial `HANDSHAKE_DONE` confirmation.
    fn establish(retry: bool) -> Self {
        let now = 0.0;
        let server_identity = Identity::from_seed(SERVER_SEED);
        let client_identity = Identity::from_seed(CLIENT_SEED);
        let hello = hello_value();
        let hello_bytes = serde_json::to_vec(&hello).expect("serialize hello");

        let mut client_hs =
            ClientHandshake::new(client_identity.clone(), server_identity.id, &hello_bytes)
                .expect("create client handshake");
        let mut server_hs = ServerHandshakes::new(
            server_identity.clone(),
            RetryTokens::from_secret(RETRY_SECRET, 10_000.0),
        );

        // Step 1: Client emits 1,200-byte padded INITIATION without a retry token.
        // Padding prevents unauthenticated UDP traffic amplification before address validation.
        let mut first = [0u8; lim::INITIATION_BYTES];
        client_hs
            .initiation(&[], &mut first)
            .expect("encode initial initiation");

        let mut response = server_hs
            .initiation(
                &first,
                "client",
                "client",
                retry,
                &hello,
                now,
                |_session_id| false,
            )
            .expect("server initiation reply");

        // When `retry` is requested, the server replies with `PACKET::RETRY` carrying a
        // stateless 24-byte HMAC token, and the client re-sends its initiation with the token.
        if retry {
            assert_eq!(client_hs.receive(&response), Event::Retry);
            let mut next = [0u8; lim::INITIATION_BYTES];
            client_hs
                .initiation(client_hs.token(), &mut next)
                .expect("encode retry initiation");
            // Fixed initiation fields (first 54 bytes: type, version, session, ephemeral, random)
            // remain identical across retries so `transcript1` stays stable regardless of which
            // initiation the server answers (`assert.deepEqual(next.subarray(0,54), first.subarray(0,54))`).
            assert_eq!(
                &next[..lim::INITIATION_FIELDS],
                &first[..lim::INITIATION_FIELDS]
            );
            response = server_hs
                .initiation(
                    &next,
                    "client",
                    "client",
                    true,
                    &hello,
                    now,
                    |_session_id| false,
                )
                .expect("server response after retry token");
        }

        // Step 2: Client verifies the server's signed RESPONSE and produces its signed CONFIRM.
        assert_eq!(client_hs.receive(&response), Event::Established);
        let confirm = client_hs.confirm().to_vec();
        let completed = server_hs
            .confirm(&confirm, "client", now)
            .expect("server confirms client handshake");

        // Both ends agree on the client's Ed25519 SPKI identity hash and the server's hello JSON.
        // Mirrors `assert.equal(session.client.id, identity(9).id)` and `assert.deepEqual(JSON.parse(native.serverHello()), hello)`.
        assert_eq!(completed.client_id, Identity::from_seed(CLIENT_SEED).id);
        assert_eq!(completed.client_id_hex, client_identity.id_hex());
        let parsed_server_hello: serde_json::Value =
            serde_json::from_slice(&client_hs.server_hello).expect("parse server hello");
        assert_eq!(parsed_server_hello, hello);

        // A duplicate RESPONSE must return Event::Duplicate and replay the exact same
        // CONFIRM ciphertext without reusing the handshake AEAD nonce.
        assert_eq!(client_hs.receive(&response), Event::Duplicate);
        assert_eq!(client_hs.confirm(), confirm.as_slice());

        // Step 3: Transition handshake traffic keys into the two native `Connection` owners.
        let client_keys = client_hs.take_keys().expect("take client keys");
        let client = Connection::new(client_keys, server_addr(), now, 100.0);
        let mut server = Connection::new(completed.keys, client_addr(), now, 100.0);
        let client_obs = EndpointObserver::default();
        let mut server_obs = EndpointObserver::default();

        // Server queues and flushes HANDSHAKE_DONE (`server.confirmHandshake(); server.flush()`).
        server.confirm_handshake();
        server
            .flush(now, &mut server_obs)
            .expect("flush server handshake confirmation");

        let mut pair = Self {
            client,
            server,
            client_obs,
            server_obs,
            now,
        };
        pair.transfer(|_, _| true);
        assert!(pair.client.handshake_confirmed);
        pair
    }

    /// Transfers queued packets between the two native endpoints for up to 64
    /// passes until neither endpoint emits further packets.
    /// Mirrors `transfer(filter)` in `tests/native-transport.mjs`.
    fn transfer(&mut self, mut filter: impl FnMut(&str, &[u8]) -> bool) {
        for _ in 0..64 {
            let mut moved = false;
            // Drain client -> server ("up") datagrams and feed them into the server connection.
            // If the caller's `filter` returns false for a packet, that packet is dropped on the wire.
            let up = std::mem::take(&mut self.client_obs.packets);
            for (bytes, _) in up {
                assert!(bytes.len() <= lim::MAX_DATAGRAM);
                if filter("up", &bytes) {
                    self.server
                        .receive(&bytes, client_addr(), self.now, &mut self.server_obs);
                    moved = true;
                }
            }
            // Drain server -> client ("down") datagrams and feed them into the client connection.
            // Immediate ACKs or control responses emitted during `receive` are picked up on the next pass.
            let down = std::mem::take(&mut self.server_obs.packets);
            for (bytes, _) in down {
                assert!(bytes.len() <= lim::MAX_DATAGRAM);
                if filter("down", &bytes) {
                    self.client
                        .receive(&bytes, server_addr(), self.now, &mut self.client_obs);
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }

    /// Advances simulated time in 10 ms increments, polling both endpoints and
    /// transferring emitted datagrams through `filter`.
    /// Mirrors `advance(milliseconds, filter)` in `tests/native-transport.mjs`.
    fn advance(&mut self, milliseconds: f64, mut filter: impl FnMut(&str, &[u8]) -> bool) {
        let end = self.now + milliseconds;
        while self.now < end {
            // Step the synthetic monotonic clock by at most 10 ms per turn so ACK deadlines
            // (25 ms for reliable streams, 40 ms for datagrams), loss timers, and PTO probes
            // fire at the same cadence as in the JavaScript test harness.
            self.now = (self.now + 10.0).min(end);
            self.client
                .poll(self.now, &mut self.client_obs)
                .expect("client poll");
            self.server
                .poll(self.now, &mut self.server_obs)
                .expect("server poll");
            self.transfer(&mut filter);
        }
    }
}

/// Mirrors `Rust client authenticates the JS server, retries and repeats one confirmation`
/// in `tests/native-transport.mjs`, plus wrong-key rejection (`Event::IdentityMismatch`).
#[test]
fn native_client_authenticates_native_server_retries_and_rejects_wrong_key() {
    // Establish a full pair with `retry: true`, verifying stateless token issuance,
    // fixed 54-byte initiation prefix preservation, and duplicate confirmation replay.
    let _pair = NativePair::establish(true);

    // Wrong expected server identity must be rejected as Event::IdentityMismatch
    // once the server's AEAD-protected RESPONSE is opened, and keys must not be produced.
    // This guards against a server spoofing another server's directory listing.
    let server_identity = Identity::from_seed(SERVER_SEED);
    let wrong_server_identity = Identity::from_seed([99u8; 32]);
    let client_identity = Identity::from_seed(CLIENT_SEED);
    let hello = hello_value();
    let hello_bytes = serde_json::to_vec(&hello).unwrap();

    let mut client_hs =
        ClientHandshake::new(client_identity, wrong_server_identity.id, &hello_bytes).unwrap();
    let mut server_hs = ServerHandshakes::new(
        server_identity,
        RetryTokens::from_secret(RETRY_SECRET, 10_000.0),
    );
    let mut init = [0u8; lim::INITIATION_BYTES];
    client_hs.initiation(&[], &mut init).unwrap();
    let response = server_hs
        .initiation(&init, "client", "client", false, &hello, 0.0, |_| false)
        .unwrap();
    assert_eq!(client_hs.receive(&response), Event::IdentityMismatch);
    assert!(client_hs.take_keys().is_none());
}

/// Mirrors `all three message lanes interoperate without a native world`
/// in `tests/native-transport.mjs`.
#[test]
fn all_three_message_lanes_interoperate_between_native_endpoints() {
    let mut p = NativePair::establish(false);
    // 32 KiB payload exercises fragmented state datagrams (`FRAGMENT_BYTES = 1111`, 30 parts)
    // as well as multi-segment ordered stream framing on `Events` and `Bulk`.
    let large: Vec<u8> = (0..32_768usize).map(|i| (i % 251) as u8).collect();

    // Queue the 32 KiB binary payload on all three lanes in both directions.
    for lane in [Lane::State, Lane::Events, Lane::Bulk] {
        assert!(p.client.queue(lane, false, &large).unwrap());
        assert!(p.server.queue(lane, false, &large).unwrap());
    }
    // Queue UTF-8 text messages on `Events` (client -> server) and `Bulk` (server -> client).
    assert!(p.client.queue(Lane::Events, true, b"native text").unwrap());
    assert!(p.server.queue(Lane::Bulk, true, b"server text").unwrap());

    p.client.flush(p.now, &mut p.client_obs).unwrap();
    p.server.flush(p.now, &mut p.server_obs).unwrap();
    p.transfer(|_, _| true);
    p.advance(2000.0, |_, _| true);

    // Verify all three binary lanes delivered identical 32 KiB payloads in both directions.
    // Both `State` (via `Fragments` reassembly) and `Events` / `Bulk` (via `ReceiveStream`
    // reassembly) must reconstruct the exact 32,768-byte buffer without truncation.
    for lane in [Lane::State, Lane::Events, Lane::Bulk] {
        let down = p
            .client_obs
            .messages
            .iter()
            .find(|m| m.lane == lane && !m.text)
            .expect("client received binary lane");
        assert_eq!(down.bytes, large);

        let up = p
            .server_obs
            .messages
            .iter()
            .find(|m| m.lane == lane && !m.text)
            .expect("server received binary lane");
        assert_eq!(up.bytes, large);
    }

    // Verify UTF-8 text flag preservation across `Events` and `Bulk` streams.
    // Stream message headers encode `kind = 1` for text and `kind = 0` for binary.
    let down_text = p
        .client_obs
        .messages
        .iter()
        .find(|m| m.text)
        .expect("client received text message");
    assert_eq!(down_text.lane, Lane::Bulk);
    assert_eq!(down_text.bytes, b"server text");

    let up_text = p
        .server_obs
        .messages
        .iter()
        .find(|m| m.text)
        .expect("server received text message");
    assert_eq!(up_text.lane, Lane::Events);
    assert_eq!(up_text.bytes, b"native text");

    // Both reliable stream send buffers must have drained completely after acknowledgement.
    assert_eq!(p.client.backlog(), 0);
    assert_eq!(p.server.backlog(), 0);
}

/// Mirrors `loss on one ordered stream does not block state or the other stream`
/// in `tests/native-transport.mjs`.
#[test]
fn loss_on_one_ordered_stream_does_not_block_state_or_other_stream() {
    let mut p = NativePair::establish(false);
    let bulk = vec![73u8; 180_000];
    let events = b"an independent event";

    p.client.queue(Lane::Bulk, false, &bulk).unwrap();
    p.client.flush(p.now, &mut p.client_obs).unwrap();

    // Drop the very first packet of the bulk stream, delivering the remaining packets
    // so the server's Bulk receive stream has a missing prefix at offset 0.
    // Stream 1 (`Bulk`) is therefore blocked on byte offset 0 while later segments sit in
    // its out-of-order reassembly map.
    let first = std::mem::take(&mut p.client_obs.packets);
    assert!(first.len() > 2);
    for (bytes, _) in first.into_iter().skip(1) {
        p.server
            .receive(&bytes, client_addr(), p.now, &mut p.server_obs);
    }

    // Unreliable state datagrams bypass stream congestion control and arrive immediately.
    // Meanwhile `Events` uses stream 0, whose reassembly cursor starts at 0 independently
    // of stream 1 (`Bulk`).
    p.client.queue(Lane::Events, false, events).unwrap();
    p.client.queue(Lane::State, false, b"new state").unwrap();
    p.client.flush(p.now, &mut p.client_obs).unwrap();
    p.transfer(|_, _| true);

    assert!(p.server_obs.messages.iter().any(|m| m.lane == Lane::State));

    // Reliable lanes share congestion control, so first release that window via ACKs.
    // The missing bulk prefix must not hold the independent event stream.
    p.advance(40.0, |_, _| true);
    assert!(p
        .server_obs
        .messages
        .iter()
        .any(|m| m.lane == Lane::Events && m.bytes == events));
    assert!(!p.server_obs.messages.iter().any(|m| m.lane == Lane::Bulk));

    // Advancing further allows loss recovery to retransmit the dropped prefix and
    // complete the 180,000-byte Bulk message.
    p.advance(6000.0, |_, _| true);
    let delivered_bulk: Vec<&[u8]> = p
        .server_obs
        .messages
        .iter()
        .filter(|m| m.lane == Lane::Bulk)
        .map(|m| m.bytes.as_slice())
        .collect();
    assert_eq!(delivered_bulk, vec![bulk.as_slice()]);
    assert!(p.client.stats.retransmitted > 0);
}

/// Mirrors `reliable messages converge under deterministic bidirectional packet loss`
/// in `tests/native-transport.mjs`.
#[test]
fn reliable_messages_converge_under_deterministic_bidirectional_packet_loss() {
    let mut p = NativePair::establish(false);
    // Queue 60 variable-length messages (`1500 + i * 3` bytes) in both directions so
    // stream frames span multiple packets and exercise range-based selective ACKs.
    let mut expected = Vec::with_capacity(60);
    for i in 0..60usize {
        let bytes = vec![i as u8; 1500 + i * 3];
        p.client.queue(Lane::Events, false, &bytes).unwrap();
        p.server.queue(Lane::Bulk, false, &bytes).unwrap();
        expected.push(bytes);
    }

    // Drop every 17th upstream packet (starting at 1) and every 19th downstream packet
    // (`direction === 'up' ? ++up % 17 !== 1 : ++down % 19 !== 1`).
    // Because both data packets and ACK-carrying packets pass through `filter`, this
    // exercises both RACK-style packet/time-threshold loss detection and PTO probes.
    let mut up = 0usize;
    let mut down = 0usize;
    let mut filter = |direction: &str, _bytes: &[u8]| -> bool {
        if direction == "up" {
            up += 1;
            up % 17 != 1
        } else {
            down += 1;
            down % 19 != 1
        }
    };

    p.client.flush(p.now, &mut p.client_obs).unwrap();
    p.server.flush(p.now, &mut p.server_obs).unwrap();
    p.transfer(&mut filter);
    p.advance(10_000.0, &mut filter);

    let up_events: Vec<Vec<u8>> = p
        .server_obs
        .messages
        .iter()
        .filter(|m| m.lane == Lane::Events)
        .map(|m| m.bytes.clone())
        .collect();
    let down_bulk: Vec<Vec<u8>> = p
        .client_obs
        .messages
        .iter()
        .filter(|m| m.lane == Lane::Bulk)
        .map(|m| m.bytes.clone())
        .collect();

    // Every message must arrive in exact stream order with zero residual stream backlog.
    assert_eq!(up_events, expected);
    assert_eq!(down_bulk, expected);
    assert_eq!(p.client.backlog(), 0);
    assert_eq!(p.server.backlog(), 0);
}

/// Mirrors `authentication and replay checks cannot advance a valid packet away`
/// in `tests/native-transport.mjs`.
#[test]
fn authentication_and_replay_checks_cannot_advance_a_valid_packet_away() {
    let mut p = NativePair::establish(false);
    p.server.queue(Lane::State, false, b"one message").unwrap();
    p.server.flush(p.now, &mut p.server_obs).unwrap();

    // Flip the final bit of the AEAD authentication tag so decryption fails.
    // Because `Replay::accepts` is checked before opening and `Replay::accept` is only
    // called after ChaCha20-Poly1305 verification succeeds, an attacker forging or
    // corrupting a packet with sequence number N cannot burn N in the sliding window.
    let (packet, _) = p.server_obs.packets.pop().expect("emitted state packet");
    let mut bad = packet.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;

    // 1. Forged tag fails authentication without committing the packet number to Replay.
    assert!(!p
        .client
        .receive(&bad, server_addr(), p.now, &mut p.client_obs));
    // 2. The genuine packet with the same sequence number still succeeds and delivers.
    assert!(p
        .client
        .receive(&packet, server_addr(), p.now, &mut p.client_obs));
    // 3. Replaying the genuine packet a second time is rejected by the replay bitmap.
    assert!(!p
        .client
        .receive(&packet, server_addr(), p.now, &mut p.client_obs));

    let received: Vec<&[u8]> = p
        .client_obs
        .messages
        .iter()
        .map(|m| m.bytes.as_slice())
        .collect();
    assert_eq!(received, vec![b"one message".as_slice()]);
    assert_eq!(p.client.stats.auth_failures, 1);
    assert_eq!(p.client.stats.duplicates, 1);
}

/// Mirrors `key phases rotate in both directions while ordered messages remain exact`
/// in `tests/native-transport.mjs`.
///
/// Exercises both ordered stream delivery across 40 messages and explicit wire-level
/// `DATA_PHASE` key rotation with previous-phase straggler acceptance below `phase_start`.
#[test]
fn key_phases_rotate_and_ordered_messages_remain_exact() {
    let mut p = NativePair::establish(false);
    let expected: Vec<String> = (0..40).map(|i| format!("message {i}")).collect();

    // Exchange 40 ordered messages simultaneously in both directions, advancing 30 ms per step
    // so selective ACKs and stream window updates interleave with new data transmissions.
    for msg in &expected {
        let bytes = msg.as_bytes();
        p.client.queue(Lane::Events, false, bytes).unwrap();
        p.server.queue(Lane::Bulk, false, bytes).unwrap();
        p.client.flush(p.now, &mut p.client_obs).unwrap();
        p.server.flush(p.now, &mut p.server_obs).unwrap();
        p.transfer(|_, _| true);
        p.advance(30.0, |_, _| true);
    }
    p.advance(1000.0, |_, _| true);

    let up_events: Vec<String> = p
        .server_obs
        .messages
        .iter()
        .filter(|m| m.lane == Lane::Events)
        .map(|m| String::from_utf8(m.bytes.clone()).unwrap())
        .collect();
    let down_bulk: Vec<String> = p
        .client_obs
        .messages
        .iter()
        .filter(|m| m.lane == Lane::Bulk)
        .map(|m| String::from_utf8(m.bytes.clone()).unwrap())
        .collect();

    assert_eq!(up_events, expected);
    assert_eq!(down_bulk, expected);

    // Also verify explicit key-phase rotation on the wire: when a peer seals packets
    // across rotated AEAD keys (`send.next()`) and toggles `DATA` / `DATA_PHASE` in both
    // directions, each receiving `Connection` rotates its receive key phase and delivers
    // the payload while still accepting previous-phase stragglers below `phase_start`.
    let server_identity = Identity::from_seed(SERVER_SEED);
    let client_identity = Identity::from_seed(CLIENT_SEED);
    let hello = hello_value();
    let hello_bytes = serde_json::to_vec(&hello).unwrap();
    let mut client_hs =
        ClientHandshake::new(client_identity, server_identity.id, &hello_bytes).unwrap();
    let mut server_hs = ServerHandshakes::new(
        server_identity,
        RetryTokens::from_secret(RETRY_SECRET, 10_000.0),
    );
    let mut init = [0u8; lim::INITIATION_BYTES];
    client_hs.initiation(&[], &mut init).unwrap();
    let resp = server_hs
        .initiation(&init, "client", "client", false, &hello, 0.0, |_| false)
        .unwrap();
    assert_eq!(client_hs.receive(&resp), Event::Established);
    let completed = server_hs
        .confirm(client_hs.confirm(), "client", 0.0)
        .unwrap();
    let client_keys = client_hs.take_keys().unwrap();

    // Derive phase-0 and phase-1 AEAD keys for both the server and the client directions.
    let server_send_phase0 = completed.keys.send.clone();
    let server_send_phase1 = server_send_phase0.next().unwrap();
    let client_send_phase0 = client_keys.send.clone();
    let client_send_phase1 = client_send_phase0.next().unwrap();
    let mut client_conn = Connection::new(client_keys, server_addr(), 0.0, 100.0);
    let mut server_conn = Connection::new(completed.keys, client_addr(), 0.0, 100.0);
    let mut client_obs = EndpointObserver::default();
    let mut server_obs = EndpointObserver::default();

    // Helper that frames a single `FRAME::DATAGRAM` and seals it under the given key and phase byte.
    // The 9-byte header carries `pkt_type` (`DATA` or `DATA_PHASE`), little-endian `receiver`,
    // and little-endian sequence `number`.
    let seal_datagram = |aead: &bfe_transport::crypto::Aead,
                         pkt_type: u8,
                         receiver: u32,
                         number: u32,
                         payload: &[u8]|
     -> Vec<u8> {
        let mut plain = [0u8; 128];
        let mut w = Writer::new(&mut plain);
        w.byte(f::DATAGRAM as u8).unwrap();
        w.byte(0).unwrap();
        w.varint(payload.len() as u64).unwrap();
        w.span(payload).unwrap();
        let plain_len = w.at;

        let mut pkt = vec![0u8; d::BYTES + plain_len + lim::TAG];
        pkt[d::TYPE] = pkt_type;
        pkt[d::RECEIVER..d::RECEIVER + 4].copy_from_slice(&receiver.to_le_bytes());
        pkt[d::NUMBER..d::NUMBER + 4].copy_from_slice(&number.to_le_bytes());
        pkt[d::BYTES..d::BYTES + plain_len].copy_from_slice(&plain[..plain_len]);
        let (hdr, tail) = pkt.split_at_mut(d::BYTES);
        let tag = aead
            .seal(hdr, number, &mut tail[..plain_len])
            .expect("seal datagram");
        tail[plain_len..plain_len + lim::TAG].copy_from_slice(&tag);
        pkt
    };

    // Server -> Client: packet 0 (phase 0), packet 2 (phase 1 rotated), packet 1 (phase 0 straggler).
    // Packet 2 advances `phase_start` to 2 and moves the old key into `previous`, allowing packet 1
    // (`1 < phase_start`) to open under `previous`.
    for (aead, kind, seq, msg, at) in [
        (
            &server_send_phase0,
            p::DATA as u8,
            0,
            &b"s-phase0-first"[..],
            10.0,
        ),
        (
            &server_send_phase1,
            p::DATA_PHASE as u8,
            2,
            &b"s-phase1-rotated"[..],
            20.0,
        ),
        (
            &server_send_phase0,
            p::DATA as u8,
            1,
            &b"s-phase0-straggler"[..],
            25.0,
        ),
    ] {
        let pkt = seal_datagram(aead, kind, client_conn.local, seq, msg);
        assert!(client_conn.receive(&pkt, server_addr(), at, &mut client_obs));
    }

    // Client -> Server: packet 0 (phase 0), packet 2 (phase 1 rotated), packet 1 (phase 0 straggler).
    // Exercises the identical key-phase rotation and straggler retention in the reverse direction.
    for (aead, kind, seq, msg, at) in [
        (
            &client_send_phase0,
            p::DATA as u8,
            0,
            &b"c-phase0-first"[..],
            10.0,
        ),
        (
            &client_send_phase1,
            p::DATA_PHASE as u8,
            2,
            &b"c-phase1-rotated"[..],
            20.0,
        ),
        (
            &client_send_phase0,
            p::DATA as u8,
            1,
            &b"c-phase0-straggler"[..],
            25.0,
        ),
    ] {
        let pkt = seal_datagram(aead, kind, server_conn.local, seq, msg);
        assert!(server_conn.receive(&pkt, client_addr(), at, &mut server_obs));
    }

    // Verify both peers delivered all three messages in arrival order across the key rotation.
    let client_payloads: Vec<&[u8]> = client_obs
        .messages
        .iter()
        .map(|m| m.bytes.as_slice())
        .collect();
    assert_eq!(
        client_payloads,
        vec![
            b"s-phase0-first".as_slice(),
            b"s-phase1-rotated".as_slice(),
            b"s-phase0-straggler".as_slice()
        ]
    );

    let server_payloads: Vec<&[u8]> = server_obs
        .messages
        .iter()
        .map(|m| m.bytes.as_slice())
        .collect();
    assert_eq!(
        server_payloads,
        vec![
            b"c-phase0-first".as_slice(),
            b"c-phase1-rotated".as_slice(),
            b"c-phase0-straggler".as_slice()
        ]
    );
}

/// Mirrors `authenticated malformed ACKs close cleanly and an ordinary close reaches the peer`
/// in `tests/native-transport.mjs`, and also verifies oversized frame bounds refusal.
#[test]
fn authenticated_malformed_acks_oversized_frames_and_ordinary_close_behave_exact() {
    // 1. Authenticated packet containing two ACK frames is rejected with PROTOCOL close (code 2),
    //    and the CLOSE frame propagates to the peer with code 2.
    //    We retain a clone of the server's initial send AEAD key (`server_send`) from the
    //    handshake so we can seal a malformed packet carrying duplicate ACK frames.
    {
        let mut p = NativePair::establish(false);
        let server_identity = Identity::from_seed(SERVER_SEED);
        let client_identity = Identity::from_seed(CLIENT_SEED);
        let hello = hello_value();
        let hello_bytes = serde_json::to_vec(&hello).unwrap();
        let mut client_hs =
            ClientHandshake::new(client_identity, server_identity.id, &hello_bytes).unwrap();
        let mut server_hs = ServerHandshakes::new(
            server_identity,
            RetryTokens::from_secret(RETRY_SECRET, 10_000.0),
        );
        let mut init = [0u8; lim::INITIATION_BYTES];
        client_hs.initiation(&[], &mut init).unwrap();
        let resp = server_hs
            .initiation(&init, "client", "client", false, &hello, 0.0, |_| false)
            .unwrap();
        assert_eq!(client_hs.receive(&resp), Event::Established);
        let completed = server_hs
            .confirm(client_hs.confirm(), "client", 0.0)
            .unwrap();
        let server_send = completed.keys.send.clone();
        p.client = Connection::new(client_hs.take_keys().unwrap(), server_addr(), 0.0, 100.0);
        p.server = Connection::new(completed.keys, client_addr(), 0.0, 100.0);

        // Client sends packet 0 so largest=0 is a valid sent packet number.
        p.client.queue(Lane::State, false, b"sent").unwrap();
        p.client.flush(0.0, &mut p.client_obs).unwrap();
        p.transfer(|_, _| true);

        // Craft an authenticated packet from the server containing two ACK frames for packet 0.
        // `Connection::frames` allows at most one ACK frame per packet to bound ACK table walks.
        let ack_frame = [f::ACK as u8, 0, 0, 0, 0];
        let mut body = [0u8; 10];
        body[..5].copy_from_slice(&ack_frame);
        body[5..10].copy_from_slice(&ack_frame);
        let pkt_num = 1u32;
        let mut pkt = vec![0u8; d::BYTES + body.len() + lim::TAG];
        pkt[d::TYPE] = p::DATA as u8;
        pkt[d::RECEIVER..d::RECEIVER + 4].copy_from_slice(&p.client.local.to_le_bytes());
        pkt[d::NUMBER..d::NUMBER + 4].copy_from_slice(&pkt_num.to_le_bytes());
        let tag = server_send
            .seal(&pkt[..d::BYTES], pkt_num, &mut body)
            .unwrap();
        pkt[d::BYTES..d::BYTES + body.len()].copy_from_slice(&body);
        pkt[d::BYTES + body.len()..].copy_from_slice(&tag);

        // Receiving the duplicate-ACK packet closes the client connection with PROTOCOL (code 2)
        // and immediately flushes a signed/sealed CLOSE frame back to the server.
        assert!(p
            .client
            .receive(&pkt, server_addr(), 5.0, &mut p.client_obs));
        let client_closed = p
            .client
            .closed
            .expect("client closed on duplicate ACK frame");
        assert_eq!(client_closed.code, close_code::PROTOCOL as u64);
        assert!(!client_closed.remote);

        // Deliver the client's emitted CLOSE packet to the server and verify code 2.
        p.transfer(|_, _| true);
        let server_closed = p.server.closed.expect("server received CLOSE frame");
        assert_eq!(server_closed.code, close_code::PROTOCOL as u64);
        assert!(server_closed.remote);
    }

    // 2. Ordinary close with code 0 and reason "done" reaches the peer intact.
    //    Mirrors `q.native.close(0, 'done', q.now); q.transfer(); assert.equal(q.closed[0]?.reason, 'done')`.
    {
        let mut q = NativePair::establish(false);
        q.client.close(0, b"done");
        q.client.flush(q.now, &mut q.client_obs).unwrap();
        q.transfer(|_, _| true);
        let closed = q.server.closed.expect("server received close");
        assert_eq!(closed.code, 0);
        assert_eq!(closed.reason(), b"done");
        assert!(closed.remote);
    }

    // 3. Oversized state message and oversized reliable stream message are refused and
    //    close the connection with close_code::PROTOCOL (code 2); oversized wire datagrams
    //    (> MAX_DATAGRAM) are dropped before decryption.
    {
        let mut r = NativePair::establish(false);
        // Any UDP datagram exceeding MAX_DATAGRAM (1,200 bytes) is rejected at the packet boundary.
        let oversized_datagram = vec![0u8; lim::MAX_DATAGRAM + 1];
        assert!(!r
            .client
            .receive(&oversized_datagram, server_addr(), r.now, &mut r.client_obs));

        // A state message exceeding MAX_FRAGMENTS * FRAGMENT_BYTES (32 * 1,111 = 35,552 bytes)
        // cannot be represented in the fragment header and triggers a protocol close.
        let oversized_state = vec![0u8; lim::MAX_FRAGMENTS * lim::FRAGMENT_BYTES + 1];
        assert!(r
            .client
            .queue(Lane::State, false, &oversized_state)
            .is_err());
        r.client.flush(r.now, &mut r.client_obs).unwrap();
        assert_eq!(
            r.client.closed.expect("closed on oversized state").code,
            close_code::PROTOCOL as u64
        );

        // A reliable stream message exceeding MAX_MESSAGE (1,048,576 bytes) is refused by
        // `SendStream::write` and triggers a protocol close.
        let mut s = NativePair::establish(false);
        let oversized_stream = vec![0u8; lim::MAX_MESSAGE + 1];
        assert!(s
            .client
            .queue(Lane::Events, false, &oversized_stream)
            .is_err());
        s.client.flush(s.now, &mut s.client_obs).unwrap();
        assert_eq!(
            s.client.closed.expect("closed on oversized stream").code,
            close_code::PROTOCOL as u64
        );
    }
}

/// Mirrors `idle peers exchange keepalives and close after the authenticated idle bound`
/// in `tests/native-transport.mjs`.
#[test]
fn idle_peers_exchange_keepalives_and_close_after_authenticated_idle_bound() {
    let mut p = NativePair::establish(false);
    // Over 5,000 ms of idle time with packet delivery enabled, keepalive PING frames
    // fire every 1,000 ms (`KEEPALIVE_MS = 1000`), keeping the session alive.
    p.advance(5000.0, |_, _| true);
    assert!(p.client.stats.sent >= 5);
    assert!(p.client.closed.is_none());

    // Dropping all packets for 21,000 ms exceeds `IDLE_MS = 20000` and closes with `IDLE` (code 1).
    p.advance(21_000.0, |_, _| false);
    let closed = p.client.closed.expect("client closed after idle timeout");
    assert_eq!(closed.code, close_code::IDLE as u64);
}

// Harness for `ServerEndpoint` driving a real loopback UDP socket.
// Records admitted session client IDs and echoes every received lane message back to the sender.
struct LiveServerHarness {
    outbox: Vec<(Vec<u8>, SocketAddr)>,
    session_client_ids: Vec<String>,
    received: Vec<(u32, Lane, bool, Vec<u8>)>,
    hello: serde_json::Value,
}

impl LiveServerHarness {
    fn new(hello: serde_json::Value) -> Self {
        Self {
            outbox: Vec::new(),
            session_client_ids: Vec::new(),
            received: Vec::new(),
            hello,
        }
    }
}

impl ServerSink for LiveServerHarness {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        // Queue outgoing server datagrams for transmission over `server_socket`.
        self.outbox.push((bytes.to_vec(), to));
    }

    fn on_hello(&mut self) -> serde_json::Value {
        // Provide the server's application hello JSON signed into the handshake RESPONSE.
        self.hello.clone()
    }

    fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
        // Accept every authenticated client in this interoperability fixture.
        AdmitDecision::accept()
    }

    fn on_session(
        &mut self,
        _session_id: u32,
        client: &ClientInfo,
        _remote: SocketAddr,
        _rtt: f64,
    ) {
        // Record the authenticated client's hex identity so we can assert reconnects share one ID.
        self.session_client_ids.push(client.id_hex.clone());
    }

    fn on_message(&mut self, session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
        // Record each incoming lane message so the test loop can echo it back on the same lane.
        self.received.push((session_id, lane, text, bytes.to_vec()));
    }

    fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {
        // Path migration is not triggered in this loopback socket test.
    }

    fn on_close(&mut self, _session_id: u32, _closed: Closed) {
        // Session teardown callbacks are observed via `ServerEndpoint` session counts.
    }
}

/// Mirrors `native UDP sockets authenticate, exchange lanes and reconnect with one identity`
/// in `tests/native-transport.mjs`.
#[test]
fn native_udp_sockets_authenticate_exchange_lanes_and_reconnect_with_one_identity() {
    // Bind an OS-assigned non-blocking IPv4 loopback UDP port for the server endpoint.
    let server_socket =
        UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    server_socket.set_nonblocking(true).unwrap();
    let listen_addr = server_socket.local_addr().unwrap();

    // Server uses seed 29 (`identity(29)` in `tests/native-transport.mjs`), and client
    // reconnects twice with `CLIENT_SEED` (`identity(9)`).
    let server_identity = Identity::from_seed([29u8; 32]);
    let retry = RetryTokens::from_secret(RETRY_SECRET, 10_000.0);
    let mut server = ServerEndpoint::new(
        server_identity.clone(),
        retry,
        listen_addr.port(),
        32,
        None,
        100.0,
    );
    let hello = hello_value();
    let hello_bytes = serde_json::to_vec(&hello).unwrap();
    let mut sink = LiveServerHarness::new(hello);

    let mut now = 0.0;
    let mut recv_buf = [0u8; 1500];
    let mut echoed_count = 0usize;

    // Connect twice in sequence with the same client identity seed (`CLIENT_SEED`),
    // verifying that each session establishes over UDP, exchanges all three lanes,
    // and closes cleanly before the next session connects.
    for _connect in 0..2 {
        let mut client = ClientEndpoint::connect(
            &listen_addr.to_string(),
            Identity::from_seed(CLIENT_SEED),
            server_identity.id,
            &hello_bytes,
        )
        .expect("connect native client endpoint");

        // Pump both the non-blocking client UDP socket and the server UDP socket until
        // the 3-way handshake and `HANDSHAKE_DONE` confirmation complete.
        let mut client_messages: Vec<(Lane, Vec<u8>)> = Vec::new();
        for _step in 0..200 {
            now += 5.0;
            client
                .pump(&mut |lane, _text, bytes, _at| {
                    client_messages.push((lane, bytes.to_vec()));
                })
                .expect("pump client");
            while let Ok((n, from)) = server_socket.recv_from(&mut recv_buf) {
                server.receive(&recv_buf[..n], from, now, &mut sink);
            }
            server.poll(now, &mut sink);
            for (pkt, to) in sink.outbox.drain(..) {
                let _ = server_socket.send_to(&pkt, to);
            }
            if client.ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(client.ready(), "client socket failed to establish session");

        // Send one message on each of the three lanes (`state`, `events`, `bulk`) and verify
        // the server echoes all three back over the live UDP socket.
        for (lane, name) in [
            (Lane::State, "state"),
            (Lane::Events, "events"),
            (Lane::Bulk, "bulk"),
        ] {
            let payload = format!("socket {name}");
            assert!(client.send(lane, false, payload.as_bytes()).unwrap());
        }
        client.flush().unwrap();

        // Pump the server and client sockets until all three echoed messages arrive at the client.
        // Each server receive may yield newly reassembled lane messages in `sink.received`, which
        // are immediately queued and flushed back to the client on the matching transport lane.
        for _step in 0..200 {
            now += 5.0;
            while let Ok((n, from)) = server_socket.recv_from(&mut recv_buf) {
                server.receive(&recv_buf[..n], from, now, &mut sink);
            }
            while echoed_count < sink.received.len() {
                let (session_id, lane, text, ref payload) = sink.received[echoed_count];
                echoed_count += 1;
                server.queue(session_id, lane, text, payload).unwrap();
                server.flush_session(session_id, now, &mut sink).unwrap();
            }
            server.poll(now, &mut sink);
            for (pkt, to) in sink.outbox.drain(..) {
                let _ = server_socket.send_to(&pkt, to);
            }
            client
                .pump(&mut |lane, _text, bytes, _at| {
                    client_messages.push((lane, bytes.to_vec()));
                })
                .expect("pump client messages");
            if client_messages.len() == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // Sort the received lane names and compare against `['bulk', 'events', 'state']`.
        // Because `State` is a datagram lane and `Events`/`Bulk` are independent streams,
        // arrival order across lanes is unconstrained, so sorting the lane tags makes the
        // assertion deterministic across kernel UDP scheduling variations.
        let mut lanes: Vec<&str> = client_messages
            .iter()
            .map(|(l, _)| match l {
                Lane::Bulk => "bulk",
                Lane::Events => "events",
                Lane::State => "state",
            })
            .collect();
        lanes.sort_unstable();
        assert_eq!(lanes, vec!["bulk", "events", "state"]);

        // Cleanly close the client session before reconnecting on the next loop iteration.
        // Pumping once after `close` transmits the `FRAME::CLOSE` datagram to the server so
        // the server retires the old session before the second `ClientEndpoint::connect`.
        client.close(0, b"disconnected");
        let _ = client.pump(&mut |_, _, _, _| {});
        while let Ok((n, from)) = server_socket.recv_from(&mut recv_buf) {
            server.receive(&recv_buf[..n], from, now, &mut sink);
        }
    }

    // Both connections authenticated as `identity(9).id` and delivered 3 messages each (6 total).
    // This proves a client can disconnect and reconnect from a fresh ephemeral UDP port while
    // presenting the same persistent Ed25519 identity.
    let expected_id = Identity::from_seed(CLIENT_SEED).id_hex();
    assert_eq!(
        sink.session_client_ids,
        vec![expected_id.clone(), expected_id]
    );
    assert_eq!(sink.received.len(), 6);
}
