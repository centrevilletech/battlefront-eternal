# Security Review: Native UDP Transport (`rust/bfe-transport`)

**Scope:**
- `rust/bfe-transport` (`packet.rs`, `cookie.rs`, `reliable.rs`, `fragment.rs`, `endpoint.rs`)
- `rust/bfe-server` and `rust/bfe-client` UDP socket loops

---

## 1. Threat Model & Attack Surface

The UDP transport parses untrusted datagrams directly from the network on both server and client endpoints. The primary threats evaluated are:

1. **Memory Safety & Parser Panics**: Malformed headers, truncated payloads, integer overflows, or out-of-bounds slices causing crashes or memory corruption.
2. **UDP Reflection / Amplification DoS**: Spoofed source IP sending small `ClientHello` packets to trigger large server responses or allocate session slots.
3. **Fragment Reassembly Memory Exhaustion ("Zip-Bomb" / Stalled Fragments)**: Attacker sending single fragments claiming `frag_count = 255` across thousands of `msg_id` values to exhaust server/client heap memory.
4. **Reliable Send/Receive Queue Exhaustion**: Unacknowledged reliable messages or out-of-order sequence gaps growing unboundedly.
5. **Cross-Session Hijacking / Spoofing**: Attacker injecting packets into an existing session from a different source IP/port or guessing `session_id`.

---

## 2. Security Controls Verified

### 2.1 Zero `unsafe` & Bounds-Checked Parsing (`packet.rs`)
- `#![forbid(unsafe_code)]` is enforced at the crate root (`rust/bfe-transport`).
- `Packet::decode` enforces:
  - Minimum header length (`HEADER_LEN = 24`) and maximum datagram size (`MAX_DATAGRAM_BYTES = 1400`).
  - Exact match between `header.payload_len` and remaining slice length (`raw.len() - HEADER_LEN != payload_len` returns `PacketError::PayloadLengthMismatch`).
  - Explicit enum validation for `PacketKind` (`0x01..=0x09`).
  - Fragment sub-header bounds (`payload_len >= FRAG_HEADER_LEN`, `frag_count` within `2..=32`, `frag_index < frag_count`).

### 2.2 Anti-Amplification & Stateless Cookie Handshake (`cookie.rs`, `endpoint.rs`)
- `ClientHello` datagrams smaller than `MIN_CLIENT_HELLO_BYTES` (`64` bytes) are silently dropped before any response is generated.
- `ServerChallenge` response is `40` bytes (`24` header + `16` payload), strictly smaller than the 64-byte minimum request (amplification factor `< 0.63x`).
- No session state is allocated on `ClientHello`. The server computes a stateless SipHash-2-4 MAC (`CookieSecret`) over `(client_addr, client_nonce, bucket)` with rotating time buckets (`current` and `current - 1`).
- A session slot is allocated only after `ClientResponse` presents a valid cookie and `sessions.len() < max_sessions`.

### 2.3 Bounded Fragment Reassembly (`fragment.rs`)
- Maximum fragments per message: `MAX_FRAGMENTS = 32`.
- Maximum concurrent partial assemblies per peer: `MAX_INFLIGHT_ASSEMBLIES = 16`.
- Maximum total reassembled message size: `MAX_ASSEMBLED_BYTES = 32,768` bytes (`32 KB`).
- Stale partial assemblies are evicted after `ASSEMBLY_TIMEOUT_MS = 2,000 ms`, or oldest-first if all 16 slots are occupied.
- Duplicate fragments or mismatched `frag_count` for an active `msg_id` are rejected cleanly without corrupting state.

### 2.4 Bounded Reliable Channel (`reliable.rs`)
- Outbound unacknowledged queue is capped at `MAX_INFLIGHT_RELIABLE = 64` messages (`ReliableError::SendQueueFull`).
- Inbound out-of-order buffer only accepts packets within `[recv_next_seq, recv_next_seq + MAX_INFLIGHT_RELIABLE)` and caps single reliable message payloads at `MAX_RELIABLE_MESSAGE_BYTES = 16,384` bytes.

---

## 3. Adversarial Test Suite

Adversarial tests in `rust/bfe-transport` (`tests/transport_suite.rs` and `tests/interop.rs`) verify:
- Random/truncated/fuzz byte streams never panic `Packet::decode` or `ServerEndpoint::receive`.
- Undersized `ClientHello` (`< 64` bytes) and forged cookies are dropped with zero session allocation.
- Fragment floods (1,000+ bogus `msg_id` single fragments) stay strictly bounded at `<= 16` active assemblies and recover cleanly to deliver valid fragmented snapshots.
- Out-of-window reliable sequence attacks are ignored without memory growth.
