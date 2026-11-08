# Snapshot publication and delta delivery

The implemented profile is `snapshot-delta-v1`, declared in
`src/runtime/network-contract.js`. Local and online play use the same record
layouts. The Rust wire owner captures canonical world state, retains baselines,
selects publication recipients and budgets, encodes deltas and produces packet
bytes. JavaScript carries transport facts, external acknowledgments and packet
delivery. It does not rebuild each actor for every publication.

## Owners and boundaries

| Responsibility | Implementation |
| --- | --- |
| Canonical capture | `rust/sim/src/wire/store_source.rs`, `rust/sim/src/wire/store_capture.rs` |
| Retained publication | `rust/sim/src/wire/owner.rs`, `rust/sim/src/wire/publisher.rs`, `rust/sim/src/wire/publish_module.rs` |
| Records and codecs | `rust/sim/src/wire/codec.rs`, `rust/sim/src/wire/records.rs`, `rust/sim/src/wire/semantic.rs` |
| Ordered observations | `rust/sim/src/wire/journal.rs`, `rust/sim/src/wire/combat.rs`, `rust/sim/src/wire/shots.rs` |
| Page receive and outbound input | `rust/bfe-presentation/src/network/`, `src/runtime/page-network.js` |
| Host session | `src/runtime/snapshot-host.js`, `src/runtime/snapshot-client.js` |

The world, input, AI and wire owners have one battle lifetime. Entity references
and gameplay lives qualify sampled bodies, seats and launches. Source-point
shot, death and impact observations retain their original order; publication
must not retap them at a different point merely to simplify batching.

## Rates and transport

Authoritative simulation runs at 60 Hz. Online snapshots normally publish every
two ticks, with input sent on the same even-tick cadence. Local transport can
publish every tick. The interval is a wire field; configurable per-client rate
tiers remain planned rather than a selectable server feature.

BFE-UDP/1 carries `state` as unreliable datagrams and `events` and `bulk` as
independent reliable streams. Snapshot datagrams are at most 1,100 bytes inside
the transport's 1,200-byte packet limit. State is sent immediately rather than
queued for reliable retransmission. Congestion can skip complete publication
opportunities; the next delta still uses a baseline that its recipient has
acknowledged. Own corrections have their own bounded fallback cadence.

See [native-udp-transport.md](native-udp-transport.md) for packet protection,
flow hints, loss recovery and channel roles.

## Snapshot records

Entity snapshots use `BNS1`; own-player snapshots use `BNO1`. Headers carry the
world epoch, snapshot tick, interval, baseline tick, part information and input
acknowledgment. Large entity snapshots use the extended part header. Encoders
repack parts within the same datagram budget; receivers reject invalid counts,
indices, extents and incompatible contracts.

Entity records carry qualified identity, class and the changed quantized state.
World records carry battle, post, roster, station, tow and other published
state. Own records carry the exact correction and input state required to
continue private prediction, plus personal summaries and ordered effects.
Quantization of displayed remote state must not replace exact own-player
replay fields.

A complete acknowledged snapshot is a baseline. Shared entity parts can be
encoded once for recipients using the same baseline. Per-client own correction
and personal data retain separate output. Loss does not acknowledge an
incomplete baseline or let another client's acknowledgment advance this one.
Epoch changes, stale generations and recovery from an absent baseline have
explicit handling. Retained rings and payload arenas are bounded.

## Input receipts and prediction

Movement receipt, consumed input, discarded input and executed action/fire
receipts are different observations. Reliable edges keep their independent
receipt loops. Snapshot acknowledgment rides with input; transport packet
acknowledgment alone does not acknowledge a gameplay baseline or an action.

Rust `ClientPrediction` restores the authority checkpoint and replays pending
commands with the shared soldier, droideka, vehicle, mounted-weapon and recon
cores. Raw pointer counts preserve their command-time aim and sensitivity.
Correction smoothing and aim previews belong to presentation, not to a second
physics model. [network-replay-contract.md](network-replay-contract.md) describes
which state must survive restoration and continuation.

The hit-history owner in `rust/sim/src/combat/history.rs` bounds each client's
rewind from its input lead, playout, round trip and snap allowances, within the
retained history. It checks the current collision world as required by the
shared shot rules. See [semantic-combat.md](semantic-combat.md).

## Receive and displayed state

The page passes snapshot bytes to the Rust network owner. Baselines, entity
identity, fractional samples, interpolation and bounded extrapolation stay in
Rust through unit, hull and ordnance presentation. Host facades retain resource
and gameplay identities for consumers that need them; they do not provide an
alternate JavaScript timeline algorithm.

A new life cannot inherit an old body's cached pose or lease. Own-player state
is excluded from delayed remote sampling. Pose history, displayed smoothing and
camera state remain local presentation state. The native client's online path
uses the same network records and presentation owners, with its admitted views
described in [native-client.md](native-client.md).

## Compatibility and checks

Protocol, profile, simulation revision, correction schema and server adapter
ABI are checked before a session is playable. `build/host-runtime.json` pins
the compatible server. A schema or behavioral change that an old peer cannot
interpret requires the matched contract update; there is no per-feature
simulation fallback. See [network-contract.md](network-contract.md).

Synthetic snapshot, delta, input and prediction fixtures run without game
files. Content-backed world hashes compare state, deep digest and publication
bytes with frozen references. Network acceptance separately exercises delay,
loss, reordering, acknowledgment stalls, reconnect and crowded publications.
A byte-identical codec does not by itself qualify displayed latency, server
capacity or native host lifecycle. [host-qualification.md](host-qualification.md)
and [world-replay.md](world-replay.md) describe those separate checks.
