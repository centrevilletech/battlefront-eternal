# Native UDP Transport (`BFU1`)

`rust/bfe-transport` provides the native UDP transport used by `bfe-client` and `bfe-server`.

---

## Architecture Overview

1. **Rust Workspace Crate (`rust/bfe-transport`)**:
   - Pure-Rust implementation of the `BFU1` wire protocol, packet framing, session state machine, reliable-ordered control channel, fragment reassembly, and unreliable sequenced input/snapshot datagrams.
   - Zero `unsafe` blocks (`#![forbid(unsafe_code)]`).
   - Validated by unit, fuzz/adversarial, and loopback tests in `rust/bfe-transport`.

2. **Native Host Integration (`rust/bfe-server` and `rust/bfe-client`)**:
   - `bfe-server` and `bfe-client` bind non-blocking OS UDP sockets directly onto `bfe-transport` endpoints (`ServerEndpoint` and `ClientEndpoint`).

---

## Wire Protocol (`BFU1`, 24-byte Header)

Every UDP datagram begins with a 24-byte little-endian header:

| Offset | Size | Type | Field | Description |
|---:|---:|---|---|---|
| `0` | `4` | `[u8; 4]` | `magic` | ASCII `"BFU1"` (`0x42, 0x46, 0x55, 0x31`) |
| `4` | `1` | `u8` | `version` | Protocol version (`1`) |
| `5` | `1` | `u8` | `kind` | Packet kind (`0x01..0x09`) |
| `6` | `1` | `u8` | `flags` | Bit `0`: `FRAGMENTED` (`0x01`) |
| `7` | `1` | `u8` | `channel` | Channel ID (`0` = control/handshake, `1` = reliable ordered, `2` = unreliable input, `3` = unreliable snapshot) |
| `8` | `4` | `u32` | `session_id` | Server-assigned session identifier (`0` during `ClientHello` / `ServerChallenge`) |
| `12` | `4` | `u32` | `seq` | Monotonic per-channel sequence number |
| `16` | `4` | `u32` | `ack` | Highest contiguous/latest sequence received from peer |
| `20` | `2` | `u16` | `ack_bits` | 16-bit selective ACK window prior to `ack` |
| `22` | `2` | `u16` | `payload_len` | Byte length of payload following the 24-byte header |

### Fragment Sub-Header (`flags & 0x01 != 0`)
When a reliable control message or large keyframe snapshot exceeds the safe UDP payload MTU (`1160` bytes), it is split into up to `32` fragments, each prefixed with a 4-byte sub-header immediately after the 24-byte header:
- `msg_id: u16`
- `frag_index: u8`
- `frag_count: u8` (`2..=32`)

---

## Handshake & Anti-Amplification Cookie

To prevent IP spoofing and amplification attacks before allocating session state:

1. **`ClientHello` (`0x01`)**:
   - Client sends `client_nonce (u64)` padded to at least `64` bytes (`MIN_CLIENT_HELLO_BYTES = 64`).
   - Server ignores any `ClientHello` smaller than 64 bytes.
2. **`ServerChallenge` (`0x02`)**:
   - Server computes stateless SipHash-2-4 cookie over `(client_addr, client_nonce, time_bucket)` using its secret 128-bit key and replies with `(server_nonce, cookie)` (`16` bytes payload, smaller than the 64-byte request).
3. **`ClientResponse` (`0x03`)**:
   - Client echoes `(client_nonce, cookie)` plus its join metadata payload.
   - Server verifies the cookie (accepting current or immediately preceding time bucket) before allocating a `SessionSlot` and replying with `ServerAccept` (`0x04`).

---

## Channel Delivery Semantics

| Message Type | Channel | Delivery Mode | Notes |
|---|---|---|---|
| Handshake (`Hello`, `Challenge`, `Response`, `Accept`, `Disconnect`) | `0` | Stateless / Connection control | Cookie-verified before session creation |
| Join / Welcome / Match Control / Reliable Events | `1` | **Reliable Ordered** | Retransmitted with exponential backoff until ACKed via `ack` / `ack_bits`; delivered strictly in order |
| Client Input (`ClientInputPacket`) | `2` | **Unreliable Sequenced** | Out-of-order and duplicate packets dropped via sequence window |
| World Snapshot (`WorldSnapshot` / Delta) | `3` | **Unreliable Sequenced** (with fragment reassembly) | Latest snapshot wins; stale partial fragments evicted after `2000 ms` |

---

## Validation Commands

```bash
# Run Rust transport unit, loopback, interop, and adversarial tests
~/bfe-work/tools/rbuild.sh cargo test --locked -p bfe-transport
```
