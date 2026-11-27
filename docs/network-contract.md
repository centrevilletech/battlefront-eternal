# Engine network contract and server ABI

`rust/bfe-server` and `rust/bfe-client` share one frozen `NETWORK_CONTRACT`.
There is one required simulation revision, not independently negotiated features.

Protocol 4 runs on the native UDP transport, BFE-UDP/1 (`rust/bfe-transport`,
[native-udp-transport.md](native-udp-transport.md)). Both `bfeserver` and
`bfe-client` link `rust/bfe-transport` and `rust/sim` from the same workspace,
so a server and the clients of the same release speak the same wire. Both hellos
in the handshake carry `protocol`, `profile` and `simulation`; the server
refuses any difference with "Update required" before a `GameSession` exists, and
the client refuses a different server the same way. The transport lanes keep
their roles: `state` (unreliable datagrams, including `BNI1` input and
`BNS1`/`BNO1` snapshots, still at most 1,100 bytes), `events` and `bulk`
(independent ordered reliable streams).

The server validates its contract before creating or advertising a playable
world. Its readiness state carries the validated contract to the LAN listener,
master listing, transport hello and game session. Health checks (`/healthz`)
inspect the advertised contract as well as process liveness.

The current engine requires `bfe-sim-30`, adapter ABI 4, and own-player
correction schema 11. The server sends the verified entity registry followed by
a `snapshot-delta-v1` welcome and starts the shared snapshot publisher directly.
Local and remote worlds use the same records and the same snapshot codec
([snapshot-delta-design.md](snapshot-delta-design.md)).

The wire contract pins these interfaces between the client and the server:

- The `state`/`events`/`bulk` lane roles of BFE-UDP/1. Identities are proven
  inside its handshake.
- Unreliable input and snapshot datagrams of at most 1,100 bytes on the `state`
  lane. Input envelope magic `BNI1`; entities magic `BNS1` with the `uint32`
  snapshot tick at offset 6; own magic `BNO1`
  ([snapshot-delta-design.md](snapshot-delta-design.md)).
- Epoch 1 for a world lifetime, command/action/fire receipt semantics, and the
  message shapes used for registry, edge translation, joins, input (which
  carries the snapshot acknowledgement), snapshot publication and gameplay
  lifecycle.
- The simulation capture, codec, snapshot publisher and combat record shapes.

Every 2004 battle uses the 2004 fixed position box and u16 codes. A 2005
battle takes its box from its map and uses u24 positions when the box is wider
than the 2004 one. A larger box changes the absolute position width while
retaining the existing packet framing. A protocol 3 client would ignore the
wide position bit and read a wide datagram at the wrong offsets; protocol 4
admission refuses it before any datagram is sent. The codebook and rounding
rules are in [Snapshot and delta encoding](snapshot-delta-design.md), and
everything protocol 4 adds for the 2005 game is in
[Network contract for both games](network-two-games.md).

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
barrel records, with `FireOutsideLimits` per aimer ([vehicles.md](vehicles.md)).
A client predicting its own walker must run the same rules as the host. Map
content generation 11 carries the records.

A Battlefront II battle also writes combat displays as world schema 0x0205, the
2005 record kind 5 (a hero's combat display) at revision 2 (identity 100000 + its
slot): every living, unseated saber wielder's, and any other soldier's while a cast
or a choke shows on it and for two seconds after. A display never takes the last 32
world payloads for a new field. It holds the revision, a byte of part flags and each
part present: the melee pose of a saber wielder (34 bytes: combo state, the tick its
clip clock read zero, loop, clip length, blade mask, guard reaction, swing turn), the cast pose
of a power it casts or holds (13 bytes: weapon class hash, fire slot, the tick its
clock read zero; a flag marks it held) and the choke that holds it (4 bytes: the
clip its power names). A display with no part is 3 bytes, so a viewer sees a cast or
a choke end: a viewer never sees a world field leave, only its new content, and the
two seconds let the display without the part arrive first. The display is sent again only when a part changes, since every clock
travels as the tick at which it read zero. A Battlefront (2004) battle writes none, and a
client that does not read the schema ignores it, so it needs no simulation change.
Saber contact effects use the existing named-effect presentation event.

A Battlefront II battle also writes world schema 518, each throw-carrying hero's
throw (identity 100000 + its slot, 72 bytes): the tick its throw animation began and,
once the blade has left the hand, the launch tick, origin, direction, speed, the
outbound and return times, the spin rate and the release point in the hero's own
frame. The payload stays the same from the press to the launch and from the launch to
the catch, so the codec sends it once per press and once per launch; the viewer steps
the battle's own flight rule from it and draws the blade for every hero, in Instant
Action and online alike. A Battlefront (2004) battle writes none, and a client that
does not read the schema ignores it.

Simulation `bfe-sim-24` adds world schema 10, the state of Endor's Ewok log
traps ([live-objects.md](live-objects.md) "Ewok log traps"): each trap's phase
(down, being built back up, cocked, swinging) and the tick it began, at identity
500000 + its index, taken at once when it changes. The traps join the live
buildings, so building indices (schema 7, repair locks) shift on Endor: map
content generation 12 carries them, and client and server must both have it.

The same simulation lets walkers wade: their legs stand on the ground
under the water, not on its surface, and deep water wears walkers and the
droideka down by their classes' `WaterDamageInterval` and
`WaterDamageAmount` ([vehicles.md](vehicles.md) "Water"). A
client predicting its own walker must stand it the same way. The world
also spawns a map's placed power-ups ([live-objects.md](live-objects.md) "Placed
power-ups"), which travel as any item; map content generation 12 carries
them.

Simulation `bfe-sim-25` changes how placed turrets aim and pose, on the host
and in the client's prediction alike ([live-objects.md](live-objects.md) "Turret
views and aim", "Turret gunners"): the turret seat's aim frame and limits, one
aim point for the server and the client, rounds sent from the barrel to that
point, the gunner posed and struck at the seat transform with posed arms, and a
bot's burst that exactly fills its heat gauge. Common content generation 2
carries the Hoth dish turret gunner's clip, which the content fingerprint covers
(`/common/anims.json`).

Simulation `bfe-sim-26` takes off and lands flyers as the 2004 game does, on
the host and in the client's prediction alike ([vehicles.md](vehicles.md),
"Controls and behavior"): the key always starts a takeoff or a landing, the
landing is the lift run back at the craft's own `LandingTime` and `LandingSpeed`,
and it settles on ground flatter than 0.9375. No content changes.

Simulation `bfe-sim-27` strikes droidekas through their class's ordnance
primitives (`p_ord` and `p_rollsphere`, described in [formats.md](formats.md)),
which the host and the client's predicted rounds and hit markers both test. The
host's lag compensation leaves a round from where its shooter is, may rewind
as far as a 300 ms round trip on top of his lead and playout delay, and locks
a single-shot weapon out from when a late press was pressed
([semantic-combat.md](semantic-combat.md) "Sniper rounds online"). No content
changes.

Simulation `bfe-sim-28` uses the tick clock for barrel recoil and seeded
streams for turret and network weapon spread and grenade spin. These rules
are shared by authoritative Rust simulation and client prediction. Clients and
servers on `bfe-sim-27` must update together: a mixed join is refused with
"Update required" rather than allowing prediction to disagree. The wire format
remains protocol 3; no content generation changes.

Simulation `bfe-sim-29` brings the droideka, explosions and mines closer to the
2004 game, on the host and in the client's prediction alike
([formats.md](formats.md) "Droideka", "Explosions", "Mines and power-ups"). The
droideka's guns turn toward what its crosshair is on and its bolts leave along
them; it climbs steps and slopes and rolls by the 2004 collision rules, moves on
every tick of a walking step, and is drawn without jumps as it folds and
unfolds. Blasts measure a droideka by the sphere around its model and test the
terrain between a blast and its target as the 2004 game does, a blast that kills
a soldier on foot sends him off from it, and rounds stop on a droideka's raised
shield bubble, where their strike takes only shield. A soldier's held automatic
fire converges on his crosshair's point as a press does. Mines bounce off
soldiers and droids and ride the hull they stick to, a character's mines are
counted across his lives, and a mine's trigger measures everyone to the sphere a
blast measures him to, a hover's widened by its springs (as blasts on vehicles
now are too). Clients and servers on `bfe-sim-28` must update together: a mixed
join is refused with "Update required" rather than allowing prediction to
disagree. The wire format remains protocol 3; no content generation changes.

Simulation `bfe-sim-30` with application protocol 4 carries the 2005 game;
neither is released yet. A 2004 battle simulates, publishes and exchanges
exactly what it did under `bfe-sim-29` and protocol 3: the same input tables,
numeric adapter and decoder, the same snapshot and own-correction bytes, the
same hellos, announcements and `/info` apart from the two version values, and
the same admission on any rotation. A 2005 match names itself and its record
revisions in a `battle` descriptor in the signed server hello, and a joining
client names the match it asks for and the revisions it implements; a client
that lacks a required revision is refused for that match alone. Every 2005
record names its kind and revision on its carrier, so a reader skips an
optional kind it does not implement by the carrier's extent. Its records are
the roll and block press and release actions and the soldier's own holds, the
own-body correction `0x82`, the display field `0x0203` and a scripted battle's
mission (state, systems, notices and roster), with per-map position boxes, the whole 512-slot entity domain and up to 256 parts a
snapshot ([network-two-games.md](network-two-games.md)). A 2005 match admits
a client without its verified content to sit the battle out with chat and the
score table. Clients and servers on `bfe-sim-29` must update together: each
refuses the other with the existing version text. No content generation
changes.

Desktop releases package `bfeserver` and `bfe-client` from the same workspace.
The desktop boot updater installs the paired release before online play. The
dedicated server updater stages the same release, waits for an empty server,
then checks its advertised contract and advancing ticks before keeping the
update. During that wait, updated clients cannot join an older server. Hosts
using the standalone archive must install the matching release themselves. See
[server-updates.md](server-updates.md).
