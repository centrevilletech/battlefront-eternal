# Native UDP transport (BFE-UDP/1)

Multiplayer protocol 3 replaces WebRTC data channels with a native UDP
transport written for this game. The engine owns it (`src/net/`); the
desktop network process and `bfeserver` both run the same modules. The
renderer never opens a socket: it talks to the network process over one
MessagePort per battle (`src/runtime/udp-transport.js`).

This document describes what is implemented. Measured results appear in the
sections they concern.

## Why

WebRTC gave every connection an ICE agent, a DTLS session and an SCTP
association. SCTP's congestion window treated the game's small, constant
state stream as elastic traffic: 1% random loss collapsed the window and
queued state behind it, and the reliable channels blocked behind their own
retransmissions. Measurements found correction gaps of about a second at 1%
loss on 200 kbit/s upstream links, and roughly doubled upstream IP traffic
from SCTP acknowledgements. A 64-client test on cloud VMs showed the
per-peer WebRTC stacks queueing seconds deep.

The native transport sends each state datagram the moment it is written,
never retransmits or queues state, keeps the reliable lanes independent of
each other and of state, and acknowledges inside packets that are going out
anyway.

## Layout

| Module | Role |
| --- | --- |
| `src/net/codec.js` | Packet and frame types, QUIC variable-length integers, range sets |
| `src/net/crypto.js` | X25519, Ed25519, HKDF-SHA256, AES-256-GCM, stateless retry tokens |
| `src/net/handshake.js` | Client and server handshake state machines |
| `src/net/connection.js` | One session: protection, acknowledgement, loss recovery, lanes, timers |
| `src/net/endpoint.js` | Sockets, demultiplexing, admission, multi-path connect, hole punching, relays |
| `src/net/stun.js`, `src/net/turn.js` | STUN reflexive discovery and a TURN UDP allocation client |
| `electron/net-service.js`, `electron/net-process.js`, `electron/net-host.js` | The desktop network utility process and its MessagePorts |
| `src/runtime/udp-transport.js` | The renderer side, behind `NetworkSession` |
| server `src/udp-server.js`, `src/udp-peer.js` | bfeserver's game port and the `GameSession` peer adapter |

Electron's Node uses BoringSSL, which has no ChaCha20-Poly1305 in
`createCipheriv`; AES-256-GCM is used everywhere (hardware AES on x86-64 and
Apple silicon). Only primitives both Node and Electron provide are used.
There is no custom cipher or MAC.

## Handshake

One round trip plus a confirmation, after the SIGMA-I pattern TLS 1.3 also
follows: ephemeral X25519 for forward secrecy, and each side signing the
running transcript hash with its long-term Ed25519 identity (the existing
local identities; a server's id is the SHA-256 of its base64 public key).

```
client -> INITIATION  session id, ephemeral key, random, [retry token]; padded to 1200 bytes
server -> RESPONSE    session id, ephemeral key, random,
                      AES-GCM{identity, signature over the transcript, hello}
client -> CONFIRM     AES-GCM{identity, signature over the transcript, hello}
server -> data        HANDSHAKE_DONE (and usually the registry and welcome)
```

- The client must name the server identity it expects (listing, LAN
  discovery or `/info`); a different key on every path fails the join with
  "Server identity does not match the directory". Online, the master
  supplies the id over HTTPS; LAN discovery and `/info` are trusted on
  first use (an attacker on the path at that moment could substitute its
  own server, though never act as the player).
- Hellos carry the wire contract (protocol, profile, simulation), content
  hash and player name. Both ends compare them: an old client is refused
  with "Update required: protocol 3 / ..." inside the encrypted session.
- Keys: `HS = HKDF-Extract(transcript through RESPONSE header, X25519)`;
  handshake keys expand from `HS`; traffic keys from
  `HKDF-Extract(transcript through CONFIRM, HS)`. Labels separate every key.
  The transcript starts with the initiation's fixed fields (type, version,
  session, ephemeral key, random), not its retry token or padding, so a
  RESPONSE verifies whichever initiation it answers.
- The server accepts a CONFIRM only from the address that sent the
  initiation: a valid CONFIRM from there proves that address received the
  RESPONSE, so nobody can open a session on someone else's address.
- Ed25519 identity keys of small order or with non-canonical encodings are
  refused (OpenSSL would verify signatures from them without any private
  key). A server hello is at most 1,014 bytes, so a RESPONSE always fits one
  datagram.
- The 1200-byte initiation means no reply is ever larger than its request,
  so the server cannot amplify a spoofed flood. It also proves the path
  carries 1200-byte datagrams.
- Under load (over 64 half-open handshakes or 200 initiations per second)
  the server answers with a stateless RETRY token (HMAC over the source
  address, session and ephemeral key, valid for ten seconds) before any key
  agreement. Handshake packets are rate-limited per network (an IPv4
  address or an IPv6 /64) with least-recently-used buckets, and only after
  a packet has the size of a handshake, so spoofed floods cannot lock out
  newcomers. Half-open state is limited to 16 per network and 1,024 in
  total (a full table drops its oldest entry) and lives 5 seconds.
- A client accepts RESPONSE, RETRY and VERSION only from a path it sent an
  initiation to, keeps one retry token per path, takes at most two RETRYs
  per path, and ignores a VERSION naming its own version. A path that
  answers with another version or another server's identity is out; the
  join fails only when every path has.
- An unknown transport version gets a VERSION reply; the client reports
  "Update required".

## Packets and frames

Every datagram is at most 1200 bytes, under the 1280-byte IPv6 minimum with
room for a 4-byte TURN ChannelData header; nothing depends on IP
fragmentation. First bytes 0xb1-0xb8 keep clear of STUN (0x00-0x03) and
ChannelData (0x40-0x4f), so game, STUN and relayed traffic share one socket.

Data packets: type (key phase), receiver session id (u32), packet number
(u32), AES-256-GCM ciphertext, 16-byte tag. 25 bytes of overhead. The nonce
is the traffic IV XOR the packet number; the header is authenticated.

Frames inside: PING, ACK (largest, ack delay, ranges: QUIC's encoding),
DATAGRAM, FRAGMENT, STREAM (lane, offset, bytes), CLOSE (code, reason),
HANDSHAKE_DONE, PATH_CHALLENGE and PATH_RESPONSE.

## Lanes

The session keeps the three lanes the game already used:

- `state` is unreliable datagrams: snapshots, inputs, input and edge
  acknowledgements. Each is sent at once and never retransmitted. A message
  larger than one packet is fragmented and arrives whole or not at all.
- `events` and `bulk` are independent ordered reliable byte streams of
  length-prefixed messages (text or binary, up to 1 MiB). A loss on one
  never delays the other or the state lane.

Messages written in one task are coalesced into as few packets as fit. A
snapshot's entity part and its own datagram usually share one packet, so the
client receives a complete snapshot (the only kind it can acknowledge as a
baseline) with one packet's loss probability instead of two.

## Acknowledgement and recovery

Every outgoing packet carries an ACK frame while traffic flows, so a lost
acknowledgement is repaired by the next packet. Packets with reliable data
are acknowledged within 25 ms; state-only packets within 40 ms, which at 30
Hz each way always rides an input or snapshot rather than a separate packet.
The ack delay is reported and removed from round-trip samples (both ends run
this code), so PING is the network round trip.

Loss is declared from acknowledgements (RFC 9002): three newer packets
acknowledged, or 9/8 of a round trip elapsed. Reordering that later proves
spurious raises both thresholds (up to 16 packets and two round trips).
Lost stream bytes travel in the very next packet. A probe timeout resends
the oldest unacknowledged stream bytes only when no acknowledgement arrives.
Reliable bytes in flight follow a NewReno-style window (32 packets initial,
halved once per round trip on loss); state datagrams are never held by it.

Replay protection is a 4096-packet sliding bitmap. A forged, tampered or
replayed packet is dropped without effect. Traffic keys rotate every 2^20
packets with a key-phase bit; packet numbers never repeat under a key.

## Sessions and paths

Packets carry the receiver's session id, not an address. The newest
authentic packet from a new address (NAT rebinding, Wi-Fi to LTE) starts a
PATH_CHALLENGE to that address, and the session moves there, without a new
handshake, when the matching PATH_RESPONSE returns: one round trip. Until
then traffic stays on the old path, so a peer cannot spoof its own packets
from a victim's address to aim the stream at it (RFC 9000 section 8.2). A new
address earns at most one small challenge per 50 ms, and a resend only while
it keeps sending. Sessions keep themselves alive with
a PING after one second of silence and close after 20 seconds without an
authentic packet. A close carries its code and reason to the other end:
kicked, banned, full, update required, server shutting down.

Probing packets (only PATH_CHALLENGE, PATH_RESPONSE or padding, as RFC 9000
section 9.1 defines them) never move a session. A PATH_RESPONSE goes back on
the path its challenge came on, one per packet, within a budget for paths
other than the session's own (a burst of 64, then 128 per second). That lets
a client time other routes to its server without the server following them.

## Picking the fastest route

Routers spread flows across parallel links by hashing their addresses and
ports, and those links can differ by many milliseconds. Measured over one
ISP path to a cloud data center, the same server was 17.5, 20, 23.5, 26 or
29.5 ms away depending only on the UDP source port, and each port kept its
route; ICMP is hashed too and can land on any of them. A session from one
random port therefore paid 6 ms on average and up to 12.

- A desktop join sends its first initiations from six local ports at once
  (internet candidates only, and only the first three sends of the
  schedule). The first authentic RESPONSE picks the session's port.
- A handshake reply is a single sample, and verifying it delays the
  replies behind it, so when the server's hello lists `transport.probing`
  the client then times sixteen ports (the raced ones plus fresh ones) with
  four staggered rounds of probes and moves the session to the fastest when
  it beats its own port by a millisecond. The server validates the new
  address before sending there; the old port stays open meanwhile. Unused
  ports close within about half a second.
- In 12 sessions per mode over that ISP path, one random port gave a median
  round trip of 24.05 ms (worst session 29.89); raced and probed ports gave
  18.11 ms (worst 18.50), against a fastest wire route of 17.5 ms. Through a
  relay on the server's own host the figures were 24.49 and 18.25 ms: the
  relay itself costs about 0.15 ms on the same route.

## Connecting

- **LAN and JOIN IP:** the network process sends the initiation straight to
  the address (UDP on the game port, 18451 by default). No master, signaling
  connection or consent is involved. `/info` stays HTTP on the same port
  number over TCP.
- **ONLINE:** the page asks the network process for candidates (public host
  addresses and the STUN-reflexive address from the relay), sends them to the
  server in an `offer` through master rendezvous, and receives the server's
  `answer`: its public host and reflexive addresses, and its private ones
  last. The server punches toward the client's candidates at the same time.
  The client tries every candidate at once and the first authentic RESPONSE
  wins, with direct paths given a 60 ms lead over relayed ones. Payloads use
  the master's existing message types; the master needed no change.
- **Online candidates:** the server answers with its public and reflexive
  addresses; its private ones only when the player's reflexive address is
  the server's own (the same NAT), so the LAN layout is not disclosed to
  everyone. It punches only toward public player addresses. The network
  process drops loopback and link-local addresses from an online answer and
  keeps at most two private ones, so a server cannot aim a player's
  initiations at the player's own machine or network.
- **Relay:** a server with `relayPolicy: auto` behind NAT, or `always`,
  allocates a TURN relay per player on coturn with the master's short-lived
  per-pair credentials, permits only that player's public addresses, and
  trickles the relayed address as a `candidate`. At most maxPlayers + 8
  relays exist at once, and one whose player has not joined within 30
  seconds is released. The player sends plain UDP
  to it; only the server speaks TURN, over a bound channel (4 bytes of
  overhead). A player who launches with `relay=always` allocates its own
  relay with its credentials, so the server only sees the relay.

## Desktop process model

`electron/net-host.js` forks the network utility process on demand and gives
each battle a fresh `MessageChannelMain`: one end to the utility process, the
other to the page (forwarded by the preload script as a window message,
since ports cannot cross the context bridge). Game packets never pass
through the main process or wait behind the render thread. The utility
process holds the identity key; the renderer can no longer request any
identity signature. The page is treated as the less trusted side: at most
four battle ports are open, each connects once and gathers once, connects
and gathers each allow a burst of six and then one every 3.3 seconds,
timeouts are clamped to 2 to 30 seconds, and closing a port cancels its
connect. A compromised page therefore cannot use the network process to
flood or scan. Messages from one socket callback share one port
message, and each carries the network process's receipt time, which the
arrival clocks use instead of the time the frame loop got to it.

## Server

bfeserver binds one endpoint (IPv4 and IPv6 sockets) on the game port for
the life of the service; rotations keep the socket. LAN, JOIN IP, online and
HOST players all arrive there. Admission checks the battle state, contract,
bans and capacity before a `GameSession` exists; a reconnect replaces a
stale session of the same identity at once. One network (an IPv4 address or
IPv6 /64, loopback exempt) holds at most `maxPlayersPerAddress` sessions
(default 8), since identities cost nothing to make. `UdpPeer` gives `GameSession` the
interface it already used. Its backlog hint is the socket send queue plus,
while the round trip exceeds the path's minimum by more than 120 ms (until
it is back within 50 ms), a skip on alternate snapshots; the next snapshot
is still encoded against the acknowledged baseline.

The game port runs on a network thread of its own (bfeserver's
`network-worker.js`): sockets, encryption, acknowledgements, relays and every
player's `UdpPeer` and `GameSession`. Inputs and snapshots cross between it
and the world worker over a dedicated port, so no packet waits on the main
thread, which keeps admission policy (bans are asked for across threads; the
endpoint admits asynchronously), rendezvous, `/info` and the operator. At
eight players on a two-core quota this cut the measured round trip by about
1.15 ms (3.9 to 2.75 ms within one datacenter), the worst state delay (p99)
by about 30% and CPU-quota throttling by about 90%. `networkThread: false`
in the server config keeps the game port on the main thread.

The server has one dependency left (`ws`, for master rendezvous): no
node-datachannel, werift, werift patch or OpenSSL binary. The listing's
transport fingerprint is now the SHA-256 of the identity key.

## Limits

What a single peer can make the other end hold or do is bounded, each limit
closing the session with a protocol error when crossed:

- one ACK frame per packet, at most 64 ranges, never acknowledging a packet
  number not yet sent;
- out-of-order stream bytes are merged into runs as they arrive: at most
  1,024 runs and 4,096 pieces per stream, nothing more than 4 MiB past the
  delivery point;
- at most 4 MiB written but unacknowledged per stream (a peer that keeps the
  session alive without acknowledging is closed);
- 16 queued PATH_RESPONSEs, 32 fragments per datagram, 16 datagrams in
  reassembly, 4,096 packets in flight, 512 queued datagrams;
- malformed STUN or TURN datagrams are dropped, never thrown into a socket
  callback.

[native-udp-security-review.md](native-udp-security-review.md) lists the
review behind these and the risks accepted.

## Operations

- Open **UDP 18451** inbound (both address families) on every server host and
  provider firewall, alongside TCP 18451 for `/info`. WebRTC reached servers
  through ICE on ephemeral ports or the relay; the native transport uses the
  game port. A host with a public address but a blocked UDP port advertises
  that address and does not fall back to a relay (with `relayPolicy: auto` a
  relay is allocated only when the host is behind NAT), so its players time
  out. Use `relayPolicy: always` if the port cannot be opened.
- Docker deployments keep host networking: players' real source addresses
  matter for admission, roaming and relay permissions.
- The desktop HOST uses UDP and TCP 18461 and loopback admin port 18460;
  `BFE_HOST_GAME_PORT` and `BFE_HOST_ADMIN_PORT` move them when another
  program holds either.
- coturn needs no change: the native transport uses its UDP allocations and
  refreshes permissions every 240 s and channels every 540 s.

## Compatibility

Protocol 3 is not compatible with protocol 2 in either direction. Deploy the
engine and server together, then publish clients, as for any contract
change. A protocol 2 client reaching a protocol 3 server cannot be told why
(it speaks WebRTC); it times out and is updated by the desktop updater.
