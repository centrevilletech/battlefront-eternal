# Mounted engine contract and server adapter ABI

`src/runtime/network-contract.js` exports one frozen `NETWORK_CONTRACT`.
The renderer, desktop identity bridge and mounted engine use that source.
There is one required simulation revision, not independently negotiated features.

Protocol 3 is the native UDP transport, BFE-UDP/1 (`src/net/`,
[native-udp-transport.md](native-udp-transport.md)). The engine owns the
transport as it owns the record codecs: bfeserver imports `src/net/` from the
mounted engine, so a server and the clients of the same engine release speak
the same wire. Both hellos in the handshake carry `protocol`, `profile` and
`simulation`; the server refuses any difference with "Update required" before
a `GameSession` exists, and the client refuses a different server the same way.
The renderer-facing lanes keep their roles: `state` (unreliable datagrams,
including BNI1 input and BNS1/BNO1 snapshots, still at most 1,100 bytes),
`events` and `bulk` (independent ordered reliable streams).

The server must load and validate this manifest before creating or advertising a
playable world. Its worker-ready message carries the validated contract to the
LAN listener, master listing, transport hello and game session. A missing
manifest, unknown manifest version, unsupported adapter ABI or disagreement with
the engine's own-correction schema must fail readiness. Do not fall back to a
server-local simulation string. Health checks must inspect the advertised
contract as well as process liveness.

The current engine requires `bfe-sim-27`, adapter ABI 4, and own-player
correction schema 11. The server sends the verified entity registry followed by
a `snapshot-delta-v1` welcome and starts the shared snapshot host directly.
Local and remote worlds use the same records and the same snapshot codec
([snapshot-delta-design.md](snapshot-delta-design.md)).

Adapter ABI 4 pins these interfaces between the engine and the server:

- The state/events/bulk lane roles of BFE-UDP/1. Identities are proven inside
  its handshake.
- Unreliable input and snapshot datagrams of at most 1,100 bytes on the `state`
  lane. Input envelope magic `BNI1`; entities magic `BNS1` with the uint32
  snapshot tick at offset 6; own magic `BNO1`
  ([snapshot-delta-design.md](snapshot-delta-design.md)).
  `tests/network-contract.test.js` pins the entities magic, the tick offset
  and the size limit.
- Epoch 1 for a world lifetime, command/action/fire receipt semantics, and the
  worker message/API shapes used for registry, edge translation, joins, input
  (which carries the snapshot acknowledgement), snapshot publication and
  gameplay lifecycle.
- The loaded engine capture, codec, snapshot host and combat adapter function
  shapes. Record bodies and correction schemas remain engine-owned; the server
  compares the runtime correction export with `ownCorrectionSchema` in the
  manifest.

A change to any server-owned assumption requires an adapter ABI change and a
coordinated compatible server. Merely accepting an arbitrary engine revision is
not safe. A new engine-owned record body can use the same adapter ABI when the
server still treats it through those unchanged interfaces, while the mandatory
simulation revision rejects clients that cannot decode it.

Deploy the ABI 4 engine and adapter as a matched pair, then publish clients.
Unsupported ABI changes fail closed and require an atomic deployment; they must
never advertise a healthy mixed contract.

## Simulation revisions

Each revision is a mandatory contract change: clients that cannot run it must
update before joining.

Simulation 19 carries exact sparse state for every own weapon slot, including
reload, charge, switching, melee, and mounted/recon weapons. Remote weapon
presentation omits default-valued fields and cannot overwrite own replay state.
Input and own-correction view ticks retain their fractional part. Hit-history
interpolates same-life poses without crossing death or teleport boundaries;
projectile catch-up retains the existing 48-tick limit and checks current walls.
Bolt launch/terminal payloads are 130/82 bytes: fractional float64 launch time,
integer execution tick, and primary/secondary channel identity. Prediction matches
sound, every pellet and its impact by channel and execution tick. Old clients must
update; those record bodies remain engine-owned.

Simulation 21 batches several changes: ground correction schema 10 carries
hover pitch/roll rates and support; world schema 9 carries creature grabs.
Protocol 3 and adapter ABI 4 use native UDP. Content map generation 9 includes
hover springs, walker foot bottoms and soft-skinned animated props. Clients
and the dedicated server must import that content and run the matched engine.

Simulation `bfe-sim-22` extends entity snapshot part counters beyond 15.
Both host and client must support the extended header. Earlier clients must
update before joining; transport protocol 3 and adapter ABI 4 are unchanged.

Simulation `bfe-sim-23` stands walkers on their leg pairs (pitch, roll and
height from foot rays) and fires their guns by the original game's aimer and
barrel records, with `FireOutsideLimits` per aimer (docs/vehicles.md). A
client predicting its own walker must run the same rules as the host. Map
content generation 11 carries the records.

Simulation `bfe-sim-24` adds world schema 10, the state of Endor's Ewok log
traps (docs/live-objects.md "Ewok log traps"): each trap's phase (down, being
built back up, cocked, swinging) and the tick it began, at identity 500000 +
its index, taken at once when it changes. The traps join the live buildings,
so building indices (schema 7, repair locks) shift on Endor: map content
generation 12 carries them, and client and server must both have it.

The same simulation lets walkers wade: their legs stand on the ground
under the water, not on its surface, and deep water wears walkers and the
droideka down by their classes' `WaterDamageInterval` and
`WaterDamageAmount` (docs/vehicles.md "Water", `src/vehicle-water.js`). A
client predicting its own walker must stand it the same way. The world
also spawns a map's placed power-ups (docs/live-objects.md "Placed
power-ups"), which travel as any item; map content generation 12 carries
them.

Simulation `bfe-sim-25` changes how placed turrets aim and pose, on the host
and in the client's prediction alike (docs/live-objects.md "Turret views and
aim", "Turret gunners"): the turret seat's aim frame and limits, one aim point
for the server and the page, rounds sent from the barrel to that point, the
gunner posed and struck at the seat node with posed arms, and a bot's burst
that exactly fills its heat gauge. Common content generation 2 carries the
Hoth dish turret gunner's clip, which the content fingerprint covers
(`/common/anims.json`).

Simulation `bfe-sim-26` takes off and lands flyers as the 2004 game does, on
the host and in the client's prediction alike (docs/vehicles.md, "Controls and
behavior"): the key always starts a takeoff or a landing, the landing is the
lift run back at the craft's own `LandingTime` and `LandingSpeed`, and it
settles on ground flatter than 0.9375. No content changes.

Simulation `bfe-sim-27` strikes droidekas through their class's ordnance
primitives (`p_ord` and `p_rollsphere`, described in docs/formats.md), which
the host and the client's predicted rounds and hit markers both test. The
host's lag compensation leaves a round from where its shooter is, may rewind
as far as a 300 ms round trip on top of his lead and playout delay, and locks
a single-shot weapon out from when a late press was pressed
(docs/semantic-combat.md "Sniper rounds online"). No content changes.
