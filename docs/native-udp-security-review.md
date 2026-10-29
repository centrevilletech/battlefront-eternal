# Native UDP transport security review (2026-10-16)

Scope: the BFE-UDP/1 transport (protocol 3) in the engine and server
repositories, before its first release:
`src/net/` (codec, crypto, handshake, connection, endpoint, STUN, TURN), the
desktop network process (`electron/net-service.js`, `net-process.js`,
`net-host.js`, preload and main hand-off), the renderer transport
(`src/runtime/udp-transport.js`), and bfeserver's game port, admission and
rendezvous (`src/udp-server.js`, `udp-peer.js`, `online-server.js`).

## Method

Three independent reviews ran in parallel, each with its own threat model
and proof-of-concept scripts against the real modules:

1. Cryptographic protocol: transcript, signatures, key schedule, nonces,
   replay window, key update, retry tokens, version and retry spoofing.
2. Parsing and denial of service: every parser and state machine an
   unauthenticated sender, a malicious authenticated peer (identities are
   free) or a malicious server can reach.
3. Trust boundaries: renderer to network process, server admission and
   rendezvous, tooling and build overrides, and what the WebRTC design gave
   that the rewrite might have lost.

Every confirmed finding was fixed, covered by a regression test
(`tests/net-hardening.test.js`, plus updated protocol, transport and server
tests), and then re-checked by rerunning the reviewers' own scripts against
the fixed code. Before and after numbers below come from those scripts.

## Result

The cryptographic core held: no finding against confidentiality, integrity
or authentication of an established session. The confirmed problems were
reflection (sending a session's traffic to an address nobody proved it
owns), resource exhaustion, one process crash, and the renderer being able
to drive the privileged network process without limits.

| ID | Severity | Finding | Before | After |
| --- | --- | --- | --- | --- |
| H1 | High | A session moved to any address that delivered one authentic packet, so a client could spoof its own packets from a victim's address and point the server's stream at it | 94 KB to the victim in 3 s from 986 bytes sent | 68 bytes (two path challenges); the session stays put |
| H2 | High | CONFIRM was accepted from any source, so a session could be opened on a victim's address | 118 KB to the victim in 3 s | 0 bytes |
| H3 | High | Out-of-order stream reassembly was quadratic in segments; tiny scattered frames froze the server's single thread | 160,000 segments (1.4 MB sent): 30 s frozen | 29 ms, and the session is closed at the limit |
| H4 | High | ACK processing scanned every packet in flight for every range of every ACK frame | 20.75 ms of server CPU per 1 KB packet (48 packets/s fill a core) | The hostile packet closes the session; the costliest valid pattern is about 63 µs per packet |
| H5 | High | A malformed TURN datagram threw out of the relay socket callback and ended the process (bfeserver or the desktop network process) | Process exit | Dropped and counted |
| H6 | High | A compromised renderer could make the network process send unlimited UDP anywhere (connect, gather, relay) | 5.2 Mbit/s of 1200-byte datagrams at loopback from 200 ports, no bound | 4 battle ports, one connect and one gather each, a burst of 6 then one every 3.3 s |
| M1 | Medium | 4,096 one-byte spoofed packets filled the handshake rate-limit table and locked out every new address; each refused packet scanned the table | New joins refused; 85 µs per packet | No state from short packets; per-network least-recently-used buckets; 1.9 µs per packet; newcomers admitted |
| M2 | Medium | One IPv6 /64 could fill all 1,024 half-open handshakes (real addresses, valid retry tokens) and starve new clients | 1024/1024, legitimate client unanswered | 16 per network; a full table drops its oldest entry; legitimate client answered |
| M3 | Medium | The client re-sent its 1200-byte initiation to the source of any RETRY, unlimited; a spoofed RETRY could also stall a join until the server's state expired | 200 RETRYs (1.8 KB) reflected 240 KB; join stalled 12 s | 0 bytes reflected; the join is not stalled |
| M4 | Medium | Free identities could take every seat from one address | All seats | 8 sessions per network on bfeserver (configurable) |
| M5 | Medium | A malicious server could steer a joining client's initiations at the client's own loopback or LAN | Any address, 12 paths | Online answers drop loopback and link-local, allow two private addresses, 8 paths |
| M6 | Medium | A peer that kept the session alive but never acknowledged made the sender buffer reliable data without bound | 5.7 MB and rising | 4 MiB per stream, then the session closes |
| M7 | Medium | bfeserver punched any address a player named, including loopback, private and link-local ones (for example 169.254.169.254) | 7 datagrams per target | Public addresses only |
| M8 | Medium | Relays were allocated per offer, so cycling identities held coturn allocations and master credential requests | 768 allocations, 640 live after 60 s | At most maxPlayers + 8; unadmitted relays released after 30 s |
| L1 | Low | One reply on one candidate (VERSION, or another server's identity) ended the whole multi-candidate join | Join failed | That path fails; the join fails only when every path has |
| L2 | Low | Small-order Ed25519 keys verified constant signatures without any private key on OpenSSL (bfeserver), giving a shared keyless identity | Accepted | Refused, with non-canonical encodings |
| L3 | Low | A large server hello could make a RESPONSE larger than one 1,200-byte datagram | 1,210 bytes | Hello at most 1,014 bytes; bfeserver caps the name at 100 characters |
| L4 | Low | The online answer disclosed the server's private interface addresses (LAN, Docker, CGNAT, ULA) to every offering identity | Disclosed | Only to a player behind the server's own NAT |
| L5 | Low | A packet of PATH_CHALLENGE frames queued one response each without bound | 100 responses | At most 16 queued |
| L6 | Low | Refused sessions kept a table slot for 2 s and counted toward capacity | Counted | Capacity counts live sessions; at most 256 draining |
| L7 | Low | The preload posted battle ports to any origin (`*`) | `*` | The page's own origin |
| L8 | Low | The test page bridge let any local process open sessions as any identity directory on disk | Any directory | Random token and attached directories only |
| L9 | Low | `BFE_HOST_SERVER_SOURCE` could bundle a local server into a CI release | Allowed | Refused when `CI` is set |
| L10 | Info | A large packet-number jump cleared the replay bitmap one bit at a time | Up to 4,095 steps | A word at a time |

Found while testing the fixes: a second connect on the same battle port
cleared the network process's reference to the first one, so closing the
port could not cancel it. Fixed; closing a port now cancels its connect.

## What changed in the protocol

These change protocol 3 on the wire before its first release; both ends
ship together as before.

- **Path validation (H1).** A packet from a new address starts a
  PATH_CHALLENGE to it; the session moves only when the matching
  PATH_RESPONSE comes back (RFC 9000 section 8.2). One challenge is
  outstanding at a time, a new address is challenged at most every 50 ms,
  and a challenge is resent only while that address keeps sending and a
  probe timeout has passed. NAT rebinding costs one round trip.
- **CONFIRM source (H2).** The server accepts a CONFIRM only from the
  address the initiation came from.
- **Transcript (M3).** The handshake transcript covers the initiation's
  fixed fields (type, version, session, ephemeral key, random) rather than
  the whole datagram, so a RESPONSE stays verifiable whichever initiation
  (with or without a retry token) it answers. The token and padding carry
  no meaning beyond admission. RESPONSE and CONFIRM are still covered whole.
- **Replies.** A client accepts RESPONSE, RETRY and VERSION only from a
  path it sent an initiation to, keeps one retry token per path (tokens bind
  the address the server saw), takes at most two RETRYs per path, and
  ignores a VERSION naming its own version.
- **Limits.** One ACK frame per packet, at most 64 ranges, never above the
  highest packet number sent; at most 1,024 out-of-order runs and 4,096
  pieces per stream, nothing beyond 4 MiB past the delivery point, at most
  4 MiB unacknowledged per stream.
- **Identity keys (L2).** Ed25519 keys whose y coordinate is 0, 1, -1 or
  plus or minus the order-8 point's (whatever the sign of x), or whose
  encoding is non-canonical, are refused. The order-8 constant was checked
  with independent curve arithmetic.

## Accepted risks

- **JOIN IP and LAN trust on first use.** The expected server id comes from
  plain-HTTP `/info` or LAN discovery, so an attacker on the path at that
  moment can substitute its own server (it still cannot act as the player).
  ONLINE is anchored by the master over HTTPS. Pinning ids after first use
  would close this; not implemented.
- **Unauthenticated RETRY and VERSION.** Now accepted only from addresses
  the client tried, with a bounded number per path. An on-path attacker can
  still disrupt a join, as it can by dropping packets.
- **Sybil identities.** Identities cost nothing, so bans by identity can be
  evaded by making a new one (unchanged from protocol 2). The per-network
  seat cap raises the cost from one address to several.
- **Residual reflection by a malicious listed server.** Each joining client
  may still send at most 8 paths times 13 initiations of 1,200 bytes to
  public addresses that server names, once per join.
- **A malicious relay operator** can inject datagrams on a relayed path;
  they must still authenticate to have any effect.
- **Key update corner cases:** a receiver that misses an entire 2^20-packet
  key phase cannot decrypt again (about 9.7 hours of loss, far past the 20 s
  idle timeout); packet-number exhaustion (2^32) closes without telling the
  peer; the previous receive key lives for one phase; forgery attempts are
  not counted against AES-GCM's integrity limit (2^52, unreachable here).
- **Stable identity on direct joins.** A JOIN IP target learns the player's
  public identity, as any server does.

## Properties that hold

From the cryptographic review, rechecked after the changes above:

- Transcript coverage without encoding ambiguity (fixed-length fields
  before each variable one), now over the initiation's fixed fields and all
  of RESPONSE and CONFIRM; traffic keys derive through CONFIRM.
- Server and client signatures use distinct labels, bind both ephemerals,
  both session ids and the signer's hello; identity proofs travel under
  handshake keys from the key exchange. No identity misbinding or unknown
  key share; the client reveals its identity only after verifying the
  server.
- Forward secrecy, key confirmation in both directions, no key-compromise
  impersonation.
- HKDF implemented correctly, every label distinct and directional.
- Nonces never repeat: each handshake key seals one message (duplicates
  are resent byte for byte), data packet numbers strictly increase and stop
  short of 2^32, and every key phase has fresh keys.
- The replay window matched a reference model over 240,000 randomized
  checks and again after its change (`tests/net-hardening.test.js`).
- X25519 low-order points are refused on OpenSSL and BoringSSL; retry
  tokens are HMACs with a per-process secret, compared in constant time.
- No downgrade: no algorithm negotiation, and the version byte is in the
  signed transcript.
- `tests/net-protocol.test.js` passes under Node 22 and Electron's
  BoringSSL.

The trust review also confirmed: the identity key never reaches the
renderer, which can no longer obtain any signature over chosen bytes (the
protocol 2 `proof`, `directConnect` and `directSignal` actions are gone); a
reconnect can only replace the same proven identity's session; established
sessions cannot be hijacked off path; `relayPolicy: always` reveals only the
relay; admission checks battle state, contract, bans and capacity before a
`GameSession` exists.

## Verification

- The engine and server test suites pass with the fixes, including 18 new
  hardening tests.
- Real Electron two-client multiplayer (preload, network utility process,
  native UDP) passed over a direct IPv4 path.
- The reviewers' proof-of-concept scripts, rerun against the fixed code,
  give the "After" column above.
