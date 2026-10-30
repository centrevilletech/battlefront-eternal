# Snapshot and delta state delivery: design

Status: implemented. Profile `snapshot-delta-v1` (`src/runtime/network-contract.js`,
[network-contract.md](network-contract.md)) delivers state this way online and
in local play. This document is the design behind it, with its rationale and
its measurement gates. Where a detail here differs from the code (constants,
cache sizes), the code is authoritative. The "per-actor path" below is the
earlier `entity-state-v1` delivery that this design replaced.

## Decisions

- Replace per-actor pacing with snapshots delta-encoded against the
  acknowledged baseline.
- Keep JavaScript and the one shared simulation; no Rust or assembly rewrite.
- Build a single 30 Hz rate first; per-client 15, 30 and 60 Hz rates follow
  once the 30 Hz path passes its gates.

This design replaces per-actor paced state delivery (`entity-state-v1`) with
the classic model of Quake 3, Source and 2004-era shooters:

- The server sends each client a snapshot every publication interval.
- Snapshots are delta-encoded against the last snapshot that client
  acknowledged.
- The client draws remote actors from an interpolation buffer.

The simulation, prediction, reconciliation, predicted shots, input edges and
the client to server input path are kept. The goal is fewer moving parts, not
new ones.

## Why

The per-actor path scheduled every record independently:

- importance and refresh intervals of 3, 6, 12 or 30 ticks
- deadlines and service head starts
- a token budget, then a second pacer on the main thread
- admission and drop callbacks with refund-once accounting
- heartbeats, plus per-packet membership ACKs that never become baselines for
  poses

Every record is absolute. A lost or dropped packet means its actors wait for
their next turn. The client then hides the gaps with coasting, braking, wall
rays, late-bracket offsets, retained-latest merging and pending-seat retries.

A capture of a client on an LTE link showed the result:

- The link was not the limit: 892 kbit/s down with headroom.
- The server held 60 Hz.
- Moving soldiers within 30 m were sampled **12 ticks apart at the median
  and 27 at p99**, against a nominal interval of 3.
- 75.6% of their rendered poses had no future sample.
- The pacer recorded 15,202 drops against 21,199 admissions.

The simulation is not the bottleneck: 3.1 ms p50 and 5.0 ms p99 of a
16.7 ms tick for 32 + 32. The delivery model is. This design changes the
model and keeps the language and the shared simulation.

## Principles

1. **Every relevant entity is in every snapshot.** No per-actor rationing. A
   congested client skips whole snapshots; individual actors never do.
2. **The only baseline is a complete snapshot the client acknowledged.** A
   lost datagram needs no bookkeeping, because the next snapshot is encoded
   against an older acknowledged baseline.
3. **Each datagram is useful on its own.** A lost datagram costs only its
   contents, never the rest of the snapshot.
4. **Encode once, send many.** Entity parts depend only on (baseline tick,
   snapshot tick), so clients that share a baseline share bytes.
5. **One code path.** Single player through `LocalTransport` uses the same
   codec.

## Rates

| Item | Value | Notes |
| --- | --- | --- |
| Simulation | 60 Hz | Unchanged |
| Snapshot | 30 Hz (every 2 ticks) for every client | The interval is a wire field, so later rate tiers need no format change. Local play uses every tick |
| Own correction | every snapshot (30 Hz) | The per-actor path's nominal was 40 Hz |
| Client input | unchanged (one datagram per 2 ticks online) | Gains `snapshotAck`, loses entity receipts. Upload is unaffected |

**Per-client rates (15, 30 and 60 Hz) are a follow-up**, not part of the
single-rate design. They multiply the hard cases:

- ring span per rate
- playout per rate
- encode fan-out
- step-down oscillation
- a rewind window that rewards the lowest rate

They are not wrong in principle. Source's `cl_updaterate` does the same
thing. They should be built only once the single-rate stream passes its
gates. "Follow-up: per-client rates" below lists what they need. All clients
send and receive on even ticks, with no per-client stagger.

## Wire model

### Datagrams

The maximum datagram is 1100 bytes, which fits a BFE-UDP/1 packet (1200
bytes) with its 25 bytes of overhead
([native-udp-transport.md](native-udp-transport.md)). A snapshot is sent to
each client as a small number of independent datagrams:

| Datagram | Content | Shared or per client |
| --- | --- | --- |
| `own` (1, or 2 when cosmetic events overflow) | own correction, own spawn/eligibility summary, own stats when changed, then cosmetic events in the remaining space | per client |
| `entities` (1 normally, more only when the field is crowded or after a recovery) | entity table delta and world field delta | shared by every client with the same baseline |

The target is one `entities` datagram in steady play. The size test
(`tests/snapshot-size.test.js`) fails if the no-loss 32 + 32 infantry case
needs more than one.

**Header**, 20 bytes for every datagram:

```
magic        u32   'BNS1' entities, 'BNO1' own
epoch        u16
tick         u32   snapshot tick
interval     u8    ticks between snapshots for this client
baselineTick u32   0 = empty baseline
partIndex    u4 | partCount u4   (entities only; own is 0/1)
ackInput     u32   (own only; 0 in entities)
```

Simulation `bfe-sim-22` keeps this 20-byte header for up to 15 entity parts.
Larger entity snapshots set the packed part byte to zero and append little-endian
`partIndex u16` and `partCount u16` after `ackInput`, making a 24-byte header.
Counts are bounded at 256 and indices must be below the count. Own packets keep
the 20-byte header and a count of one or two. The encoder repacks large snapshots
for the extra four bytes, keeping every datagram at or below 1100 bytes. The
client rejects conflicting counts and acknowledges only after every unique
entity part and required own packet arrives.

Every datagram is decodable by itself. Each `entities` part restarts slot
coding: its first record writes an absolute slot, and the following records
write a gap from it. A vehicle, its seat records and its mount record always
go in the same part, because parts apply independently.

### Applying and acknowledging

- **Apply.** The client applies any `entities` part whose baseline it holds
  and whose epoch matches, as soon as it arrives. Interpolation can use a
  snapshot that has some parts.
- **Acknowledge.** Only a snapshot with **all** its `entities` parts **and**
  its first `own` datagram becomes a baseline, because the own-correction
  delta is encoded against the own state of the baseline tick. The client
  stores it in its ring and acknowledges the newest such tick. A second
  `own` datagram holds only cosmetic events and does not count toward
  completeness.
- **Late snapshots.** An older complete snapshot that arrives late is still
  inserted into the ring, where it can fill an interpolation hole. Gameplay
  state (`ackInput`, health, seats, summaries) is applied only from a tick
  newer than the newest already applied. A late snapshot can never move the
  input acknowledgement backward.
- **Own datagram.** The own correction is delta-encoded against the own
  state of the acknowledged baseline tick, or written absolute when the
  baseline is empty.
  - The client keeps that decoded own state in the ring entry.
  - The server keeps a per-client ring of own states with the same tick
    stamps.
  - The delta compares the encoded field values exactly (the f64 fields
    stay f64), so reconciliation sees the same numbers as the full own
    correction schema 11 gives it.
  - A lost own datagram is replaced 33 ms later.

### Rings, ticks and epochs

**Server ring**
- 128 entries, each storing its tick and epoch beside its data.
- At 30 Hz the ring covers 256 ticks (4.3 s).
- A lookup whose stored tick or epoch does not match is a miss, so
  `ring[tick % 128]` aliasing cannot return the wrong entry.

**Client ring**
- The client keeps every complete snapshot newer than
  `newestTick - 256 ticks`: the same span as the server ring, at any rate,
  stored the same way.
- The server's baseline is always inside the client's span. A client that
  receives a snapshot naming a baseline it does not hold is in a bug state.
  It logs it, acknowledges nothing older, and waits for the server's
  empty-baseline recovery.

**Acknowledgement**
- Every input datagram carries `snapshotAck {epoch u16, tick u32}`.
- The server accepts it only if:
  - the epoch matches,
  - the tick is newer than the baseline already in use,
  - and the server ring still holds that exact tick.
- Reordered or stale acks are ignored. The baseline never moves backward.
- A spawn-screen or spectating client with no input sends a 10-byte
  keepalive ack every 100 ms.

**Epoch change** (map or match change)
- Both rings are cleared.
- The server sends an empty-baseline snapshot.
- Acks from the old epoch are ignored.

**Empty baseline**
- The server sends one whenever the acknowledged tick has left its ring.
- It is a full snapshot, about 64 × 20 bytes plus the world fields, which is
  two or three parts.

### Entity records

The server keeps a net slot table of 512 entries. Each entry holds a soldier,
droideka, vehicle, turret, item or recon droid.

```
slot     u16 absolute (first record of a part) | varint gap (others)
op       u8   2 bits: update | new | removed;  6 bits: group mask
fields        groups in the class's order
```

**`new` records**
- They carry `generation u16` (wrapping), `classId u16` and `team u8`,
  followed by every group against zero.
- The encoder emits `new` whenever the slot's generation or class differs
  from the baseline, including a slot that died and was reused between the
  baseline and now.
- The client destroys the old body and spawns the new one on that op alone.
  Nothing waits for the `events` channel.
- The registry descriptor on `events` carries only the name tables (classes,
  weapons, posts), sent once at join and at each epoch.

**Other ops**
- `removed` has no fields.
- An unchanged slot costs zero bytes.
- Teleports within one life set a discontinuity bit in the position group.
  The client never infers jumps from speed.

**Field groups.** Each class has at most six groups, and the position mode
and velocity width are flags inside their own group payload, not in the op
byte.

| Class | Groups |
| --- | --- |
| soldier | position, velocity, aim, vitals (health u16), status (posture and charge, state, weapon index), fire (`fireCounter` u8) |
| droideka | position, velocity, aim, vitals + status, droideka (mode, flags, five animation values), fire |
| vehicle | position, velocity, orientation, vitals (health u16, state u8, power u8), mount (per-seat yaw/pitch, tilt, roll, walk phase), fire |
| turret | aim (yaw, pitch, gun yaw, gun pitch), vitals (health, state, team) |
| item, recon droid | position, velocity, orientation or aim, vitals |

A unit test sets every group of every class in one record and checks the
round trip.

**Quantization** uses each map's own bounds:

| Field | Encoding | Resolution |
| --- | --- | --- |
| x, z absolute | u16 over the terrain extent (2048 m on BF1 maps: 256 cells × 8 m) | 3.1 cm |
| y absolute | u16 from the terrain minimum minus 50 m up to `SetMaxFlyHeight` plus 100 m | under 1 cm on BF1 maps |
| position small delta | 3 × i8 at 2 cm against the baseline, used only when every axis fits | 2 cm |
| velocity | 3 × i8 at 0.25 m/s when every component is within ±31.75 m/s, otherwise 3 × i16 at 1/32 m/s; applies to every class, except that items always take the i16 form so a page can fly them from their records (the reader takes either form for any class) | |
| yaw | u16 | 0.0055° |
| pitch | i16 | 0.0055° |
| vehicle orientation | smallest-three quaternion in 48 bits, 3 × 15 bits plus index | |

At 30 Hz a craft at 30 m/s moves 2 m per snapshot. Once the baseline is an
RTT old it takes the absolute form, so the size budget counts vehicles as
absolute.

### Size budget (estimates; `tests/snapshot-size.test.js` measures them)

**`entities` datagram, 32 + 32, all moving**

| Content | Count | Bytes each | Total |
| --- | --- | --- | --- |
| Header | 1 | 20 | 20 |
| Moving soldiers (small delta) | 63 | about 12 | 756 |
| Moving vehicles (absolute) | 6 | about 26 | 156 |
| Status, vitals and fire changes | about 20 | 2 to 4 | 60 |
| World field changes | | | 20 |
| **Total** | | | **about 1,010** |

This is close to the 1100 cap, which is why the size test gates it. If it
fails, the first lever is dropping velocity for soldiers whose velocity
matches the position difference, followed by a second `entities` part. A
second part costs nothing extra, because each part applies by itself.

**`own` datagram**
- A full correction is about 350 bytes for a firing soldier and 550 to 700
  bytes for a pilot (schema 11, f64).
- The own-correction delta targets under 200 bytes for infantry and under 400
  bytes for a pilot. The size test measures both.

**Totals**
- About 1.3 to 1.7 KB per snapshot. At 30 Hz that is about 40 to 50 KB/s,
  roughly 320 to 400 kbit/s, plus about 4 KB/s of transport overhead.
- That is within the 2 Mbit/s download target.
- Server upload for 64 clients is about 3 MB/s, roughly 25 Mbit/s.
- A desktop HOST on a home uplink cannot serve 64 remote humans at that
  rate. The HOST's default human cap must come from its measured uplink.
  That cap is a product decision, not part of this design.

### Events

**Gameplay events** go on the reliable `events` channel and are retired
when delivered:
- damage and death feedback, kill credit
- building destroyed or rebuilt
- anything a player must not miss

**Cosmetic events** use a bounded per-client outbox. Launches, first
projectile samples, terminals and explosions remain owed until an acknowledged
snapshot carried them. They are resent after a later snapshot is acknowledged
instead, or after 30 ticks. Both own datagram parts must arrive before the
client acknowledges a split snapshot. Own-only corrections do not retire events.
Later projectile refinements and presentation events remain expendable.
Priority is own events, launches, terminals, presentation, then samples, with
nearer events first. Bolt relevance considers its path. Overflow, owed drops
and resends are counted; expiration and capacity bound the outbox.
A launch or terminal is sent in a compact form (quantized position and aim)
only when its position lies inside the snapshot bounds; one outside them
travels as its exact payload, which every client already decodes
(`encodeCosmeticEvent`). The recon droid's orbital strike leaves about 515 m
up: clamped to the default bounds' 300 m, its bolts were drawn on a path
215 m too low, under the ground and out of everyone's view.

**Budget**
- 100 shots per second field-wide, each launch about 24 bytes plus an
  impact of about 14 bytes, comes to about 3.8 KB/s for a client that sees
  all of them.
- Relevance cuts this. A client gets launches and impacts within 300 m or
  in its view, as the presentation already decides.
- The per-client cost is a copy of pre-encoded event bytes, not a
  re-encode.

**Muzzle flash and fire audio** for remote soldiers come only from launch
events. `fireCounter` drives only the firing animation, and the client
ignores a jump of more than 3 between consecutive snapshots.

### World fields

Battle, command posts, building states, tow state and per-unit weapon
presentation are typed fields in the `entities` datagram's world section,
delta-encoded like entities.

- Building and tow state (JSON in the per-actor path) start as opaque blobs
  with a version counter, then become binary fields.
- Player names and slow roster data move to `events`. Roster scores stay in
  the world fields.

## Server

### Publication every 2 ticks

1. Capture into the ring entry for this tick. Capture is shared
   (`network-entity-capture.js`), and `snapshotFromRecords`
   (`snapshot-records.js`) turns it into quantized integers, so client and
   server agree bit for bit on every baseline.
2. Group clients by acknowledged baseline. For each distinct baseline,
   encode the `entities` parts once and cache them.
   - The shared baselines are bounded (`MAX_SHARED_BASELINES` in
     `src/runtime/snapshot-host.js`) and chosen to minimize the clients'
     summed baseline age.
   - Each client uses the newest chosen baseline it has itself acknowledged
     (never one only another client holds), or an empty baseline.
   - Loss fan-out therefore costs at most that many encodes per snapshot,
     plus the empty baseline.
3. Encode each client's `own` datagram.
4. Hand everything to the session for sending.

### Flow control

The transport sends each state datagram at once and never queues or
retransmits it ([native-udp-transport.md](native-udp-transport.md)), so flow
control is the server's choice of what to send:

1. **Send-age rule.** The server does not send a datagram captured more than
   one interval ago (33 ms). This replaced the 250 ms capture-age gate and
   stays a hard rule.
2. **Backlog hint.** A client that cannot keep up skips whole snapshots. Its
   state byte budget (`stateBytesPerSecond` in
   `src/runtime/snapshot-host.js`) and the server peer's backlog hint (the
   socket send queue, and a round trip well over the path's minimum) both
   skip it.
   - Skipping is safe, because the next snapshot is still encoded against
     the acknowledged baseline.
   - A client skipped for its budget still gets its own datagram alone every
     100 ms (`OWN_FALLBACK_MS`), so own corrections never starve.

Under WebRTC the state channel also needed an SCTP packet lifetime and a
patched werift to keep queued data from outliving its use. Protocol 3 has no
SCTP queue, so neither exists.

### Adapter ABI 4

The engine host remains the single implementation used by both the server
and local play. The ABI changes are:

- Removed: `ack` (now inside `input`), `admitted`, `dropped`.
- `edgeAck` and the `entity-edge-results` messages for input edges are
  **unchanged**. Fire and action edges keep their receipt loop; only entity
  packet receipts go away.
- Contract: profile `snapshot-delta-v1`, `serverAdapterABI: 4` and the
  simulation revision, all in the engine's `src/runtime/network-contract.js`
  ([network-contract.md](network-contract.md)).
- The server and engine deploy together. The engine pins its server runtime
  in `build/host-runtime.json`, and that pin moves to the new server commit.

### Rewind window

`hit-history.js` clamped `viewTick` to `tick - 48` for everyone. The clamp is
per client:

```
tick - (that client's input lead + its playout delay maximum + 2)
```

That is 24 + 10 + 2 = 36 ticks at 30 Hz. A client cannot claim a view older
than its own pipeline justifies. The budget has since gained allowances for
the round trip and the playout clock's snap (`rewindBudgetTicks` in
`hit-history.js`; [semantic-combat.md](semantic-combat.md), "Sniper rounds
online"), still within the 48 ticks the history keeps.

## Client

### Remote presentation

- **Render time** = server tick estimate minus the playout delay.
  `PlayoutClock` stays, with an online range of 4 to 10 ticks.
- **Missing snapshots widen the delay.** In the per-actor path a lost
  snapshot measured as an on-time gap, because the gap formula subtracted
  the ticks it skipped. The arrival sample now adds one interval for each
  missing snapshot tick (a tick jump larger than the interval), so sustained
  loss holds a spare interval in the buffer.
- **Interpolation.** An entity is interpolated between the two ring
  snapshots that bracket render time: slerp for vehicles, shortest-arc for
  yaw.
- **Gaps.** If the newer bracket is missing, the entity extrapolates at its
  last velocity for at most 100 ms, then holds. With independent parts and a
  delay of at least two intervals, one lost datagram is interpolated
  through, not extrapolated.
- **Snaps.** A `new` op or the discontinuity bit snaps.
- `CorrectionSmoother` and the own-player path are unchanged.
- Recon droids are drawn between the two records that bracket render time,
  extrapolating at their last velocity for at most 6 ticks, as above.
- Items (mines and power-ups) are flown on the page instead
  (`src/runtime/network-entity-items.js`). Their flight has no randomness,
  so the page seeds a copy from an item's first record and steps it with
  the shared `Items.fly` (gravity, the ray against the page's collision,
  bounce and settle) one tick at a time as render time crosses ticks,
  drawing it between its last two ticks at render time's fraction. Each
  record steers the copy when render time reaches it: a copy more than
  5 cm (or 1 m/s) off takes the record's state behind an offset that
  fades over 80 ms; a closer one is nudged halfway, so the records'
  rounding averages out. The wire has no landed bit, so a record at rest
  (zero velocity) lands the copy where the server says, unless it already
  rests within the record's rounding of that spot, and a resting item
  never drifts or falls through ground the page lacks. An item appears
  when render time reaches its first record and goes when it reaches its
  removal, never on receipt. A render-time jump of more than 12 ticks, or
  back, starts the copy again from the newest record at or before it. The
  copy is presentation only: the server alone decides where an item ends
  up and who takes it.
- Projectiles keep their short coast (they are events).

Own prediction collides with remote bodies drawn about one playout delay in
the past. That is inherent to interpolation, unchanged by this design, and
resolved by server corrections.

## Superseded code

These parts of the per-actor path are superseded. Removing them is a separate
cleanup after the gates pass, so some are still in the tree.

**Engine:**
- `network-entity-stream.js`'s scheduler, offers, pending versions,
  heartbeats and per-record `newer()` merging
- `network-entity-importance.js`, and `EntityBudget` in
  `network-entity-budget.js` (`readStateBudget`, which the snapshot host
  uses, stays)
- forced-correction and refund logic in `entity-host.js`
- `network-entity-timeline.js`'s coast, brake, jump inference and segments
- `EntityPresentation.smooth`'s late-bracket offsets and `clampCoast`
- retained-latest and `pendingSeats`/`pendingState`/`pendingCorrection`
  retries
- tombstones
- entity packet receipts in `network-entity-sender.js` and
  `network-session.js`
- the absolute per-kind records
- the f64 combat payloads
- `presentation-backlog.js` and `network-simulator.js` (unused by the game)

`snapshot-interval.js` goes only together with updates to
`tools/prediction-timing.mjs` and `tools/vehicle-prediction-timing.mjs`,
which import it.

**Server:**
- `state-pacer.js`
- `entityPacket` admission and drop posting
- `entity-ack` handling
- `fresh-state.js` (unused)

`bulk-frame.js` goes only together with updates to
`scripts/live-server-smoke.mjs` and `scripts/load-client.mjs`, which import
it.

**Kept:**
- input edge receipts

**Tests:** the tests pinning removed behavior go with it:
- scheduling, freshness, pending, event-scheduling, importance, budget
- pacer, admission ACK
- timeline coast, presentation late-bracket

## Measurement gates

Use `tools/net-probe.mjs`, which runs production client code over the real
transport. The scenario letters are historical: C and D3 measured the portable
WebRTC server, which protocol 3 retired.

Scenarios:
- **A.** 2 clients, 32 + 32, shaped 200 kbit/s up and 2 Mbit/s down,
  25 ± 10 ms, 1% loss.
- **B.** Scenario A with 5% loss and 60 ± 30 ms.
- **D1.** 64 clients on a 2-core limit, unshaped.
- **D2.** Scenario D1 with 2% loss on every client (encode fan-out).
- **E.** Scenario A with a 1.5 s uplink stall on one client (ack stall and
  ring recovery).

| Gate | Target | Scenario |
| --- | --- | --- |
| Near-moving sample gap p95 | ≤ 4 ticks | A |
| Near-moving sample gap p99 | ≤ 6 ticks | A |
| Near-moving sample gap p95 | ≤ 6 ticks | B |
| Coast-cap fraction | < 1% | A, B |
| Visible jump > 1 m per frame | 0 outside respawn and teleport | A, B |
| Own correction gap p95 | ≤ 4 ticks | A, B |
| No-loss `entities` datagrams per snapshot, infantry | 1 | size test, A |
| State download per client | ≤ 55 KB/s | A |
| Frozen remotes after the stall | none beyond the stall plus 100 ms | E |
| Ticks per second | ≥ 59.9 | D1, D2 |
| Publication p95 | < 4 ms | D1, D2 |
| Single-player regression | identical gameplay; local suites pass | local |

**Final gate:** a playtest over an LTE link against a real server, checking
whether remote units still jump.

## Phases

The work is staged so that single player never runs a half-built path.

**0. Baseline.**
- Run scenarios A to E on the per-actor path.
- Separately, land the per-client `viewTick` clamp as its own fix.

**1. Codecs (pure engine modules).**
- Quantized snapshot struct, entity and world delta, part packing.
- Own-correction delta against the baseline's own state.
- Compact cosmetic event codec.
- Tick-and-epoch-stamped rings.
- Tests:
  - Property round trips over random states and baselines, including slot
    reuse between baseline and snapshot, class changes, epoch changes, and
    every group of every class.
  - Size test on real 32 + 32 captures from `tools/headless-world.mjs`:
    infantry, vehicles, a pilot's own datagram, 100 shots per second, and
    an empty-baseline recovery.
  - The size test fails if the no-loss infantry snapshot needs a second
    `entities` part.

**2. Host and client together, in one change.**
- Snapshot host (ABI 4), client receive, rings, interpolation and the
  playout change.
- Local play switches in the same commit, so single player never runs a
  half-built path.
- The old path's code stays in the tree until phase 5.

**3. Server.**
- `game-session.js` send-age rule, backlog hint and contract bump.

**4. Measure.**
- Gates A to E.

**5. Delete.** Not done yet: the superseded modules are still in the tree.
- Everything in "Superseded code", in its own commit, with the scripts and
  tools that import deleted modules updated in that same commit.
- Then update `docs/network-contract.md` and the server README.

**6. Follow-ups, each measured alone:**
- Per-client rates (below).
- Binary building and tow fields.
- Native code only for a profiled hot spot, compiled to WASM so client and
  server keep one simulation.

## Follow-up: per-client rates

The design allows 15, 30 and 60 Hz, chosen per player, in one match. The wire
format already carries `interval`. Building it needs:

- **Aligned ticks.** 15 Hz gets ticks divisible by 4, 30 Hz ticks divisible
  by 2, 60 Hz every tick.
- **Capture and ring sizing.** Capture runs at the highest rate present.
  The ring is sized in ticks so the lowest rate still holds 256 ticks.
- **Rewind and playout.** The per-client rewind clamp above, with each
  client's own playout maximum.
- **Fairness.** A 60 Hz player sees a peek about 100 ms before a 15 Hz
  player, as with Source's `cl_updaterate`. This is accepted as the cost of
  choice, not hidden.
- **Step-down.** A client drops one tier after 3 s of sustained congestion
  and recovers one tier after 30 s clean, never above the player's choice.
- **Visibility.** The effective rate is shown next to PING in the Tab list.
- **Gates.** D2 again with clients split across tiers.
