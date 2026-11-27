# Network contract for both games

This describes application protocol 4 and simulation revision `bfe-sim-30`,
the one protocol revision that carries battles of both games. It is not
released: protocol 4 and `bfe-sim-30` ship when the 2005 game is switched
on, as a client and server pair built from the same workspace revision.

Three things are kept apart below: what a 2004 battle exchanges (exactly what
it did before, apart from two version values), what the 2005 game's network
path implements and how far it is reachable, and the drafts that later 2005
features will fill in without another protocol change.

## Status

| Part | State |
| --- | --- |
| 2004 battles | Implemented and reachable everywhere. Every message, record and table is the protocol 3 one apart from `protocol` and `simulation` |
| Record revisions: the per-match table and offer, refusal of one match, skipping an optional record only at the revision the match advertises for it, once its carrier's kind, revision and extent check out, on every carrier | Implemented on both ends; reached by tests |
| Match descriptor in the signed hellos, admission order, role from the verified digest, bounded held messages | Implemented on both ends; reached by tests |
| Selecting a 2005 battle through the playlist, `/info`, the master listing, the LAN reply, signalling, the match announcement and rotation | Implemented on the battle selection seam and behind the 2005 availability gate, which is closed in every build (see "The gate"); reached by end-to-end tests that hand the test-only open gate to the servers they start |
| 2005 input tables (roll, block press and release, sprint, crouch and reload holds, the hold-to-block setting) | Implemented: generated tables, both senders, the server decoder per game, the authority's command layer with ordered block edges per life and combo presses from the reliable trigger edges |
| 2005 soldier body `0x82` and display field `0x0203` | Implemented: authority capture, strict readers, client restore and replay of movement and guns (an end-to-end test has a client predict its soldier as a real server moves him), the display installed per remote life; the firearm permission reaches both weapon paths. His melee, thrown blade and powers are not predicted |
| Receiver checks of 2005 records before acknowledgement | Implemented: every field of the 2005 range by the match's plan, and the complete own correction, whose life keeps its class in the match's epoch whatever order corrections arrive in |
| Capacity, admission census, keyframe budget, staging banks, 256-part snapshots, receiver recovery | Implemented; measured at a crowded battle's size, with and without a mission, and an invented space battle's |
| One verified selection: the approved loader and fingerprint, recorded admission, verified restart | Implemented: one admission and one content digest per start, before anything is bound or advertised |
| A 2005 battle played online | The battle role, the scene by the 2005 loader, the class checks and the flight ceiling in the welcome are implemented; no test plays a 2005 battle online through the scene, and no build plays the 2005 game |
| Class movement numbers | Read at admission from the prepared content the digest covers (each class's properties and its forward roll clip), on the server and, by the same rule, on a joining client; a client whose content cannot give a class its numbers refuses the battle and names the class |
| Sit-out role | Server role, routing and per-peer table cache implemented; the client's join sits out every 2005 battle it does not play (the gate closed, or no map), keeps the loading screen up with the reason, and joins the next battle on its own; driven through a mixed rotation on one port against a real server in both gate states |
| Combat checkpoint parts | Framing, routing by the match's plan and reassembly anchored to the accepted correction implemented; the combat state it carries is a draft |
| A scripted battle's mission: match state, HUD timer, personal points, objectives, critical systems, notices and the teams' rosters | Implemented: written by the authority from its script host, checked and stored by every client, read into the same mission view Instant Action's screens read; an end-to-end test has two clients play an invented mission on a real server to its own end |
| Space Assault records | Implemented: pilot and gunner controls (kind 16), starfighter chunks, seats and own correction body `0x91` (kind 17), world animation clocks (kind 19), live guided ordnance and bursts (kind 20), and Space turrets and proximity doors (kind 21), all revision 1, validated before acknowledgement and driven through the shared Space presentation and prediction path |

## A 2004 battle

For a 2004 battle, protocol 4 changes the protocol number (3 to 4) and the
simulation revision (`bfe-sim-29` to `bfe-sim-30`), and nothing else. That
holds on every rotation, including one that also plays 2005 maps:

- The hellos, the admission and its refusal texts, `/info`, the LAN reply,
  the master listing, the entity registry, the welcome and the match
  announcement keep their keys and values. There is no `families` key and no
  `battle` key.
- The input packet, the numeric input adapter, both input writers and the
  server's decoder accept exactly what they accepted: actions 0 to 20 and held
  codes 0 to 103. Actions 21 and 22 of the numeric adapter remain its local
  primary and secondary tap tags, never wire actions.
- The discovery and signed-hello readers keep their tolerance for extensions
  they do not know, including an explicit `family`, a lone `mode` and a
  `families` list of any members.
- Snapshots, own corrections, events and the journal are byte for byte the
  same. A 2004 battle uses the fixed 2004 position box, 256 working entries,
  192 world fields, 256 known identities, 512 rows and at most 32 parts a
  snapshot.
- A 2004 load never reads, checks or keeps 2005 class movement numbers, so
  they cannot refuse it.

This is proven against the released build: generated corpora of the numeric
adapter, both writers, the sender's edges, the discovery and signed-hello
readers and the admission decisions, each compared with a capture from the
released code; 2004 world digests on eight maps; and byte comparisons of the
published snapshots, own datagrams, events and journal in Instant Action and
on a dedicated server with late joins, a leave and rejoin, a map change and a
lossy, reordering path.

## Record revisions: one protocol for every later 2005 change

Protocol 4 fixes two things for good: every format a 2004 battle exchanges,
and the framing both games share (datagrams, the input packet, the world
field envelope, the own correction envelope, the checkpoint part datagram and
the reliable lanes). Everything only a 2005 battle carries is a record of
some kind, each kind has numbered revisions
(`rust/sim/src/wire/revisions.rs`), and every carrier marks each record with
its kind and revision so that a reader that does not know a kind can still
find where the record ends:

| Carrier | Direction | How a reader finds kind, revision and extent |
| --- | --- | --- |
| Shared world field | server to clients | schema `0x0200 + kind`; the payload starts with the revision (u16), the field's length is the extent |
| Own correction body | server to its player | body schema `0x80 + kind` inside the own correction envelope; the body starts with the revision, the envelope's counted body is the extent |
| Records section | client to server (2005 input packet) | bit 1 of the acknowledgement byte announces it after the acknowledgement: a count (at most 16), then per record kind (u8), revision (u16), length (u16) and body |
| Reliable record | either way | `{"type":"record","kind":K,"revision":R,"body":…}`, exactly these keys |
| Checkpoint part | server to its player | the part header names kind and revision |

The rule:

1. A 2005 match's signed server hello carries a `battle` descriptor: the
   family, mode and qualified map, the match's own token (two matches of one
   selection never share it) and its record table. The table names each kind
   the match uses once, in ascending order, with one revision (at least 1)
   and whether a client must take part in it: for a kind the server writes,
   the revision it writes; for a kind the client writes, the revision the
   server reads. The input tables, the soldier body and the sit-out notice
   are named in every match, required: a table that leaves one out or marks
   one optional is malformed, so no client can join while skipping one. A
   2004 battle's hello has no descriptor.
2. A client joining a 2005 match names in its hello the match discovery
   described (mode, map, token) and, per kind it implements, the range of
   revisions it implements: those it reads of a kind the server writes, and
   those it writes of a kind the client writes.
3. The server refuses the join with close code 3 (version) and the ordinary
   update text when a required kind's revision is outside the client's
   range: "Your game is older than this server's Battlefront II battle.
   Update Battlefront Eternal to join it.", or, when the client implements
   only newer revisions, that the server's battle runs an older version.
   Neither refusal concerns the server's other battles: the same client joins
   its 2004 battles as released.
4. The client checks the signed descriptor itself before it reads any of the
   match's messages (the same match, a table whose required kinds it
   implements), and holds what arrives until that check passes. Both ends
   then use the same plan: per kind, the revision both use, or, for an
   optional kind the client does not implement, the revision the match
   advertises for it, the only one at which a record of it is skipped.
5. A reader dispatches every record on the kind, revision and extent its own
   carrier names, read and checked first: it skips a record of an optional
   kind it does not implement, whole, by that extent, only when the carrier
   names the revision the match advertises for that kind, and a writer never
   writes one. A record of a kind the match does not name, or of another
   revision than the match's (a skipped kind's included), is refused. A multipart record (the parts
   datagram) is dispatched the same way, so a new multipart kind is skipped
   or refused like any other. Both ends take every send and receive decision
   from the one plan they computed at admission, including what a client's
   input packet may carry.
6. Revision 0 does not exist on the wire: a table, offer, records-section
   record, part or reliable record naming it is refused, even one its reader
   would skip. Drafts below are unnumbered until they are written.

Both lists travel as standard padded base64 of fixed-size rows (4 bytes per
table row: kind, revision u16, flags with bit 0 required; 5 bytes per offer
row: kind, lowest u16, highest u16), at most 32 rows, so the largest table is
172 characters and the largest offer 216; longer text is refused before it is
decoded. The largest hello any build can
send (a name of 32 four-byte characters, a digest, the longest mode and map,
an offer of all 32 kinds) is 694 bytes and the largest answer a server writes
611, both within the handshake's 1,014-byte limit; tests send both through the
real signed handshake.

A later change to a 2005 format is therefore a new revision of its kind:
servers that write it require it for their matches, clients that implement it
join them, and no 2004 battle or older 2005 match is touched. A newer client
keeps implementing the older revisions it can, so it joins servers that have
not updated. The protocol number changes again only if a 2004 format or the
shared framing changes.

| Kind | Number | Writer | This build | Required | Carrier |
| --- | ---: | --- | --- | --- | --- |
| Input tables | 1 | client | revision 1 | yes | input packet actions and held codes |
| Soldier own body | 2 | server | revision 1 | yes | own correction body `0x82` |
| Soldier display | 3 | server | revision 1 | no | world field `0x0203` |
| Combat checkpoint | 4 | server | draft | | checkpoint parts |
| Combat display | 5 | server | revision 2 | no | world field `0x0205` |
| Thrown saber | 6 | server | revision 1 | no | world field `0x0206` |
| Roster points | 7 | server | reserved | | world field `0x0207` |
| Pilot and gunner controls | 16 | client | revision 1 | yes (Space) | records section |
| Starfighter state | 17 | server | revision 1 | yes (Space) | world field `0x0211` and own body `0x91` |
| Mission's critical systems | 18 | server | revision 1 | no | world field `0x0212` |
| World animation clock | 19 | server | revision 1 | yes (Space) | world field `0x0213` |
| Space ordnance and bursts | 20 | server | revision 1 | yes (Space) | world field `0x0214` |
| Space turrets and doors | 21 | server | revision 1 | yes (Space) | world field `0x0215` |
| Mission state | 24 | server | revision 1 | yes | world field `0x0218` |
| Mission notices | 25 | server | revision 1 | no | reliable record |
| Sit-out notice | 26 | server | revision 1 | yes | reliable record |
| Sit-out score table | 27 | server | revision 1 | no | reliable record |
| Mission roster | 28 | server | revision 1 | yes | record beside the entity registry |

## Joining a 2005 match

The server hello of a 2005 match keeps the released keys (`protocol`,
`profile`, `simulation`, `content`, `fingerprint`) and adds

```json
"battle": {"family": "swbf2", "mode": "eli", "map": "swbf2/kam9",
           "match": "5eed0123456789abcdef0123456789ab",
           "records": "AQEAAQIBAAEDAQAAEgEAABgBAAEZAQAAGgEAARsBAAAcAQAB"}
```

with exactly those five keys: `mode` a playlist token of 1 to 32 lowercase
letters, digits or underscores, `map` the qualified id (`swbf2/` and 1 to 32
lowercase letters, digits or underscores), `match` exactly 32 lowercase
hexadecimal digits (the same token the match announcement and the sit-out
records carry), `records` the table. `fingerprint` is the match's verified
content digest, `bfe-content-sha256-` and 64 lowercase hexadecimal digits,
and so is `content` in a 2005 match's answer: an operator's content label
never enters it. The client hello keeps `protocol`, `profile`, `simulation`,
`content` (the client's own digest for that map, or null without it) and
`name` (control characters removed, at most 32 characters), and adds
`battle` with the same first four keys and `supports`, the client's offer.

The server's admission of a 2005 match runs in this order:

1. A protocol, profile or simulation mismatch gets the existing version
   refusal (close code 3).
2. Ban and capacity (close code 4).
3. A hello without `battle`, or one naming another match (including another
   match of the same selection): close code 4 with reason `match-changed`.
   A malformed request: close code 2.
4. A required revision the client does not read: close code 3 with the
   update text of rule 3 above.
5. The role. A client whose `content` is the match's verified digest plays.
   Any other client sits the battle out: `missing-content` without a digest,
   `content-mismatch` with another one. An operator's configured content
   label never stands in for the digest.

The client checks the signed answer before any of the match's messages is
read. It holds at most 64 messages and 256 KiB meanwhile, each reliable
message at most 64 KiB and each state datagram at most one datagram's
payload, checked before anything is copied; more is a fault. A signed answer
without a descriptor (a 2004 battle now), or one naming another match, means
the match changed while joining: the client looks for the server's current
battle again rather than reporting a fault. A table with a required kind it
does not read gives the update text. A malformed descriptor or digest is a
fault. The role follows from the verified digest: a client with the map that
finds another digest does not make its loaded world live. Then the client
installs the match's record checks and releases what it held.

A refusal for the version or for `match-changed`, or a signed answer naming
another match, starts a probe of the server's current battle, and while the
probe is pending the client does not read the signed answer again: the probe
decides. If the server now runs another version, the probe reports the
version text at once; it does not wait out its timeout. A battle started in
the meantime is joined afresh. A client sitting a match out that finds the
match changed while joining reads nothing it held and looks for the server's
current battle the same way. For a server reached through the master's
listing, the readiness probe asks through the master's signalling, whose
answer carries no version, so the probe also compares the listing's version
with this build's every five seconds: another version is final at once, and
the server is ready only once a listing has named this build's version. A
missing or unreadable listing never makes it ready.

### Playing or sitting out a 2005 battle

A client resolves the game of the battle a server names, current or
announced next, by the same rules: a game it does not know, two games, or a
next battle in a namespace it cannot load end the join before any content.
A 2005 battle, current or announced, is joined either way. The client asks
for the battle role (its hello carries its content digest) only when its
gate opens the 2005 game and it has the battle's map; otherwise its hello
carries none, it reads no 2005 content, and the server gives it the sit-out
role. A battle played loads its scene by the 2005 loader from the 2005
content folder and its content query, never the 2004 loader's query. A
battle sat out keeps the loading screen up with the reason (this build does
not play the game, the game data is missing, or it differs from the
server's), follows the match's announcement to the next battle once the
server runs it, and joins that one with no action of the player's; Escape
goes back to the server list. The server list keeps its own rule: it offers
a server whose current battle is a 2005 one only to a build that plays it.

Released clients keep their own behaviour, which no protocol 4 change can
reach. A released protocol 3 client meeting a protocol 4 server is refused
at its handshake with the ordinary version text, but its rotation and rejoin
probes wait out their own timeouts (60 seconds, two minutes for a listed
server) before the player sees it. A protocol 4 client meeting a protocol 3
server reports the older server at once. The pair that ships, protocol 4
client and server, reports a version change in the probe's first answer.

A 2005 battle is read by the 2005 loader from its prepared content, and its
verified digest is the approved 2005 fingerprint of those files for the
battle's content query (map, era, mode and mission). That query is the battle
query with the bare map and, when it names no era, the era its side fights in
(`bfe_family::swbf2::content_query`), so a map with a mode in both eras gives
every reader the same mission. The world admits exactly the mission that digest
covers, picked from the same query by the same rule. A server
starts a 2005 battle only after one verified admission: the world loads
first, on the thread that will run it, and its load computes the battle's one
content digest. A refused admission is the start's error, before any socket
is bound or any match is signed, listed or welcomed; the hello and listings
advertise the digest of the world that was admitted.

A server whose rotation names a 2005 battle needs the prepared content of the
battle it starts with. It passes over any other entry it has none for, with a
log line, when its turn comes, in memory only: the saved configuration keeps its
order. A rotation of 2004 battles is checked and scheduled as released.

A battle of a mode the script host runs (`eli`, `ass`) is admitted through its
mission selection from those same files, once: the mission whose scripts the
admission ran must be the one the digest covers, or the load is refused. Its
world thread has the 4 MiB of stack a mission's callbacks need in a debug build.
A mission whose script fails as it starts ends its battle as a draw: its world
stops stepping and takes no joins, and the service schedules the next battle.

The recorded admission (the calls a host replays to admit a world call by
call) opens with the battle's game (`movement_family`, by number), then one
call per class's movement numbers, before any other name is interned, and
records the runtime's position box and slot domain, the staging capacity and
the capture's field banks, so a replay of the recorded calls on a fresh world
rebuilds a 2005 world with the same numbers. A 2005 battle goes ready only
when every soldier class its sides carry has movement numbers.

What the stages do not hold (a 2005 map's world bounds before them; its space
battlefield, the page's random streams, the fielded bodies and the infantry
after them) belongs to the admission itself, and one function does it for a
local battle and a server's, so a server's battle has it when it starts, when it
restarts, when the rotation comes to it and when a player joins it. A soldier
outside his map's bounds dies; without them he is held to the 2004 box, which a
space battle's hangars lie outside of.

A finished, empty 2005 battle restarts as the same battle in the next epoch,
and only as that battle: the reconstructed world must have the same verified
digest, position box, registry and class movement numbers as the accepted
one before it replaces it. Content changed on disk in the meantime refuses
the restart and the accepted world stays.

A 2005 battle's welcome carries `flightCeiling`, the flight ceiling in
metres its soldiers were admitted with: a JSON number holding a finite IEEE
binary32 (magnitude at most 3.4028234663852886e38) widened exactly, as a
mission script supplies it, 200 when the mission names none. It holds for the
whole epoch: a new epoch's welcome carries its own, and nothing changes it in
between. A server refuses to load a 2005 world whose ceiling is not such a
value; a client refuses a 2005 welcome without one, and its prediction (each
class's soldier constants and every hull's flight environment) uses it in
place of the value its own files give, before any predicted tick. A
2004 welcome is the released one and has no such key.

## Sit-out (2005 matches only)

A client sitting a 2005 battle out has an authenticated connection without a
world. The server assigns it no character, team, input mailbox, snapshot
registration or prediction lease, drops state input and acknowledgements
from it, refuses gameplay calls such as `start` with `sitting-out`, and sends
no state datagrams, registry, welcome or gameplay records. It sends, as
reliable records of the match's plan:

- the sit-out notice (kind 26, required), body `{"match":…,"mode":…,"reason":…}`,
  once, in place of the registry and welcome;
- the score table (kind 27, optional): a complete table at once, then at most
  once a second while it or a player's ping changes, and the final table as
  the battle ends; each table is serialized once, and a peer whose queue
  refused one is sent it at the next pass;
- and, as released messages, global and server chat, ping, and match
  announcements.

The table's body has `match`, the authority `tick`, `phase` (0 before play,
1 in play, 2 ended), `winner` (0 unset or drawn, else one of the table's
teams, only once ended), `teams` as `[team, label, score, limit]` and at most
256 `rows` as `[slot, team, name, kills, deaths, points, human, pingMs]`
ordered by roster slot. The teams are the two armies (teams 1 and 2, their
score the reinforcement count) and every other team a row belongs to, such as
a map's native locals (their score 0), in team order, each label at most 64
UTF-8 bytes, limit 0 for none; a row's team is one of them or 0 for none.
`pingMs` is the player's measured round trip, joined by the roster slot of
the player's character, or null for a bot or an unmeasured player. The client
reads these strictly: bounded size before parsing, no control characters,
teams and winner that exist, the notice once and for the mode discovery
described, and only tables of the match its notice named, each strictly newer
by the wrapping authority tick. A record of any other kind is refused. It may
send global chat; it has no team channel.

A 2004 battle has no sit-out role: content that differs gets the released
version refusal, whatever else the rotation holds.

## Content digest

A 2005 battle's digest is `fingerprint::hash_for_family` with the 2005
family, computed once over the 2005 content folder for the battle's content
query, on the server and on a joining client alike. It covers what the
battle's gameplay consumes as the approved loader reads it: the map's
gameplay data, collision and masks, classes and animation banks, the
selected mission and its lookup order, the mission scripts, and each side's
classes, weapons and bot data. A 2004 battle's digest is the released one.

A 2005 battle's soldier classes take their movement numbers from the content
the digest covers: each class record's properties in its side's `units.json`
(its jetpack among them: jump and push, the fuel numbers, hover, its own
acceleration and its energy or fuel source) and the frame count of its
forward roll clip in the animation banks, by the rule the local 2005 battle
uses (`movement::soldier::swbf2::classes`). A 2005 soldier's body takes its
class's `Acceleration` property by the same rule, on the server and in the
client's prediction. The
server's admission reads them; nothing else supplies them. A joining client
reads them from its own copy by the same rule and, if any soldier class has
none, refuses the battle with the class and what its data lacks. Content that
differs, a roll clip of another length for instance, has another digest, so
the server gives that client the sit-out role.

A joining client's offer, its scene and its prediction come from one reading
of its files: the fingerprint keeps every file it reads, found or not, and
the 2005 loader reads those bytes and the folder only for files the digest
does not cover, so a file replaced after the offer is not what the client
plays. A client that offered its content has loaded the battle's scene when
the signed answer arrives; if the answer gives it the sit-out role, the battle
hands that connection, with the messages held while the answer was checked,
to the map-free session. The player is told the battle uses other game data
and follows the next battle as any client sitting a battle out does.

## Input actions and bindings

The 2004 tables are unchanged: `INPUT_CODES` (104 physical codes) and
`INPUT_ACTIONS` (21 actions, `jump` to `targetLock`). The 2005 tables
(input revision 1) are generated beside them from
`rust/sim/schema/transport-layout.json`:

| Input | Wire form | Games |
| --- | --- | --- |
| `roll` | durable action edge, action 21 | 2005 |
| `blockPress`, `blockRelease` | durable action edges, actions 22 and 23 | 2005 |
| power lock press on foot | durable action edge, action 20 (`targetLock`) | 2005 |
| sprint | held code 64 (`ShiftLeft`) while on foot | 2005 |
| crouch, reload held | held codes 2 and 17 (`KeyC`, `KeyR`) while on foot | 2005 |
| hold-to-block setting on | held code 1 (`KeyB`) while on foot | 2005 |

`SWBF2_INPUT_CODES` is the same 104 codes and `SWBF2_INPUT_ACTIONS` the 21
actions plus these three. A 2005 sender translates the bound controls and the
setting to these held codes on foot, whatever keys the player bound (in flight
`ShiftLeft` stays boost and the others are not sent); it appends the roll and
block edges after the numeric adapter's own actions and encodes with the 2005
action count. A block tap that begins and ends between two commands is sent as
its own press and release. A packet with an action or code outside its
battle's table fails as an unknown action or held input.

On the authority, a 2005 battle's command layer consumes a `prone` press
without effect, and a roll press while seated likewise; both are receipted so
neither repeats on a later life. A `targetLock` press on foot is a hero's
power lock: it takes a lock on the enemy nearest his aim, or lets a held one
go, as the same key does in a local battle; in a seat it stays the vehicle's.
A 2004 battle's `targetLock` is a pilot's alone. Block is held from its press edge to its
release edge, on foot only, so a short press can never be lost to a replaced
held bitmap and a tap and a hold are told apart identically on both sides.
A soldier's combo reads his block with his hold-to-block setting: with the
setting on, block lasts as long as its control is held; without it, a press
starts and ends it, as in a local battle. Hero attacks and Force powers use
primary and secondary fire, whose combo presses come from the reliable trigger
edges, so a tap between two commands counts. A combo's held jump is the held
`Space` code on foot, its sprint, crouch and reload the codes above, with
crouch, reload and jump presses also from their durable action edges. Lock-on
on foot stays in the client; if lock-on ever steers the authority it becomes
a durable edge naming its target's life, in a later input revision.

Until the 2005 game is released, a change to what its input means is made in
revision 1 in place: no released build reads it.

The 2005 mover reports after each tick whether that tick lets a firearm fire
(not while sprinting or rolling). The authority's combat and the client's
replayed weapons both apply the same answer for the same tick.

## Soldier records (revision 1)

**Soldier body `0x82`** (kind 2) is a 2005 soldier's own-body correction
inside the own correction envelope (schema 11): the revision (u16, 1), the
compact soldier body of schema 6, unchanged, then a suffix of exact
little-endian values:

- `energy:f64`, signed, within the class's energy floor and capacity;
- `moves:u8`: bit 0 sprint, bits 1 and 2 the roll (0 none, 1 forward, 2
  left, 3 right), bits 3 and 4 zero (the combat checkpoint, a later
  revision), bit 5 exhausted, bit 6 tumbling, bit 7 unsupported;
- while sprinting, `sprintTime:f64` the sprint's acceleration time, within
  zero and the class's sprint acceleration time;
- while rolling, `rollElapsed:f64` the roll's elapsed time, within zero and
  the class's roll window (`0.75 × (frames − 1) / 30` seconds), for a class
  with a roll action only;
- `landCooldown:f64`, the posture lock after a soft landing, zero or more;
- `jumpCount:u32`, the life's ground and Force jumps;
- while unsupported, `unsupportedTime:f64`, the time standing or crouching
  over no ground before falling, zero or more;
- while tumbling, `phase:u8` (1 thrown, 2 tumble, 3 landed, 4 getting up, 5
  recovering), `flags:u8` (bit 0 face down, bit 1 bounced, bit 2 died
  tumbling, other bits zero), `flyDir:u8` (0 to 3), then six doubles:
  elapsed and air time (zero or more), angle, spin rate, heading offset and
  cycle phase.

While the soldier holds a power lock on a living target, a lock tail
follows the suffix: the target's unit slot (`u16`) and life (`u32`), 6
bytes. Its length is the only sign of it: the suffix's own length follows
from its `moves` byte, and a reader takes the 6 bytes after it as the lock
or refuses any other remainder.

The suffix is 21 bytes idle and at most 96. It is the whole per-life state
the soldier step carries between ticks, so a client restores the bits the
authority stepped and replays the same rule. The reader checks every value
before anything is used: never prone or diving and a finite lock, every
number finite, the ranges above, and only the bytes the flags name; what it
accepts the writer writes back byte for byte. A whole-correction delta (mode
1) applies only between payloads of equal length; any length change sends
the absolute form.

**Display field `0x0203`** (kind 3, optional) shows a 2005 soldier to every
client, keyed by the soldier's id and life. After the revision (u16, 1):
`energy:u16` the balance as a
fraction of the class's capacity, rounded and clamped to [0, 1] (0 for a class
without energy); `flags:u8` with bit 0 sprint, bits 1 and 2 the roll and bit
3 exhausted, other bits zero; then while rolling `start:u32`, the authority
tick of the roll's first step (`tick + 1 − max(1, round(elapsed × 60))`,
wrapping), and `heading:u16`, the body's current yaw at 65,536 steps a turn.
It is 5 bytes idle and 11 rolling, with the revision. A receiver places the roll pose at its own
frame time from the start tick and the class's roll window. The field is
staged with its life every 30 ticks, and at once when the sprint, roll,
latch or life changes. When a body leaves the field, both its display field
and its unit field (schema 6) leave the snapshots; a 2004 battle keeps the
unit field of every body it ever fielded until it ends.

## Checks before acknowledgement

In a 2005 match the receiver checks every 2005 record before anything of its
snapshot or own datagram is stored, presented or acknowledged: a world field
in the 2005 range must belong to a kind the match's table names (an optional
kind the client does not implement is decoded like any field, from its
base when it is a delta, and must show the revision the match advertises for
that kind; it is then kept, never presented, as the base of its next delta),
its revision must be the match's, and the display field is read strictly. An
own correction is checked whole: the complete envelope (header, counted body,
life facts, weapons, controls, mount or remote, nothing trailing), a class
of the match's table that stays the same within one life of one unit in
whatever order the corrections arrive (the receiver keeps the newest accepted
correction's unit, life and class for its epoch, and a late correction of an
earlier tick never replaces them), and its body by schema with every split
checked: the soldier body at the match's revision against the movement
numbers of the soldier's class and the compact body it holds, or a 2004
vehicle or droideka body by its own reader. A 2004 soldier body in a 2005
match, a 2005 body of another kind, a class without numbers or any refused
record rejects the whole snapshot or datagram, which is then not
acknowledged. A late correction of an earlier life, or of the unit before a
respawn, is the server's own and is read, not refused. The soldier display
that passes is installed for the unit's life it belongs to, and a display of
an older life never replaces a newer one. A 2004 battle's receiver reads
every field as released.

## Mission records (revision 1)

A battle of a mode the script host runs has a mission: its script decides the
score, the timers, the objectives, the messages and the end. The authority
writes what the host holds; every client reads it into the mission view the
battle's screens read in Instant Action (`rust/sim/src/wire/mission.rs`,
`rust/bfe-client/src/online/mission.rs`). Numbers are little endian. A text is
its length (u8) and at most 96 bytes of UTF-8. Every reader takes its record
whole, and refuses an unknown flag, a value of something the record says is
absent, a number out of range or not finite, and a byte left over.

**Mission state, `0x0218`** (kind 24, required). Shared world fields, life the
epoch. They are the state itself, so a client that joins or resynchronizes has
the whole of it with its first complete snapshot. The body starts with a tag,
and the field's id is `tag × 256 + field`; a field at another id is refused. A
field that changed goes with the next publication.

- tag 0, the match: `flags:u16` (bit 0 over, 1 game over, 2 finished, 3 the
  script faulted, 4 and 5 the points of team 1 and 2 shown, 6 minimap hidden, 7
  command posts hidden, 8 a score target, 9 and 10 finite tickets of team 1 and
  2), `outcome:u8` (0 none, 1 victory of, 2 defeat of, 3 draw, 4 draw by a
  fault) and `team:u8`, `target:u32`, `points:[u32; 2]`, `tickets:[u32; 2]`.
- tag 1, the timer the script shows: `flags:u8` (bit 0 shown, 1 running),
  `due:u32` the authority tick it is due while it runs, `left:u32` the ticks it
  has left while it stands still, and its name. A timer that stands still stays
  on the HUD at the time it has left, as the game shows it, and goes at zero or
  when the match is over; a local world's view makes the HUD timer the same way
  from the same facts.
- tag 2, personal points: `field:u8` (at most 15), `mask:u32` over the 32
  roster slots from `32 × field`, then `points:i32` per slot of the mask.
- tag 3, objectives in the order the script added them: `field:u8` (at most 7),
  `count:u8` (1 to 8), then per objective `flags:u8` (bit 0 active, 1
  completed), `team:u8` (0 everyone, 1 to 7 a team, 255 nobody) and its text
  key. A screen shows the active ones of its player's team. A field holds
  at most 8; objectives past the 64th are not sent.
- tag 4, the players who wait for a body: `field:u8` (at most 15), `mask:u32`
  over the 32 roster slots from `32 × field` and `chosen:u32` of those who have
  chosen what to deploy as, then `due:u32` per slot of the mask: the tick from
  which the mission may give him one, past his own wait after a death, the spawn
  wave after it and his team's next slot in it, or 0 while it may already. Sent
  for every field the roster reaches, with nobody in it too. A row that only
  counts down does not change.
- tag 5, roster classes: `field:u8` (at most 15), `mask:u32` over the 32 roster
  slots from `32 × field`, then `class:u16` (non-zero registry id) per slot of
  the mask, so the scoreboard names bots by class as Instant Action does.
- tag 6, class availability: `count:[u8; 2]` (admitted class count for teams 1
  and 2, each at most 12) and `available:[u16; 2]` (bitmask in each team's
  roster order; bit `i` is set when a human could take class `i` right now, and
  bits at or past `count[t]` are zero), sent whenever any class fills or opens
  so the deployment screen greys unavailable classes on both teams from the
  same check the refusal uses.
- tag 7, the living players outside the battlefield: `field:u8` (at most 15),
  `mask:u32` over the 32 roster slots from `32 × field`, then `due:u32` per slot
  of the mask: the tick his leaving countdown runs out. Sent for every field the
  roster reaches, with nobody in it too; the tick stays the same while a
  countdown runs, so the field changes only as a player leaves or comes back.

Until the 2005 game is released, revision 1 of `MATCH_STATE` (like `INPUT`
revision 1) remains open because no released build reads it; tags 5, 6 and 7
were added in place, and once shipped any further format change will increment
the record revision.

**Critical systems, `0x0212`** (kind 18, optional), id the system's index in
the order the mission named them (at most 32), life the epoch: `index:u8`,
`flags:u8` (bit 0 alive, 1 marked for the players, 2 health known, 3 shielded,
4 placed on the HUD), `team:u8`, `place:u8` and `placeId:u16` (0 elsewhere, 1
the placed building of that index, 2 the hull of that id), `points:i32`,
health and its maximum, shield charge and capacity, and the HUD place, two f32
each (zero where not known), and its name. A rebuilt object is the same system.
Its fate, side, mark and the shield's fall go with the next publication, its
wear every 12 ticks.

**Notices** (kind 25, optional). Reliable records to each player they are for,
with the body `{"sequence":N,"tick":T,"until":U,…}`: the recipient's own rising
sequence, the authority tick it happened on, the tick it stops being worth
showing (at most 300 later), and one of `"text":key` (a message), `"hint":key`,
`"voice":sound` or `"kill":[killer or null,victim,weapon]`, where a killer or
victim is `[roster slot, human, class]` and class and weapon are ids of the
matched registry, 0 for none. A kill by an unmanned ship gun (no killer, the
weapon a ship building's) adds a fourth element, the gun's entity name (text,
empty when it has none, never `|`); a reader refuses a fourth element after a
killer. The authority chooses the audience: what the
script tells one team goes to that team's players only. A player whose queue is
full keeps his order and loses what expires or, beyond 64 waiting, the oldest. A
player who joins is sent the battle's opening, then what still shows for him,
each with its own tick and end, then what happens from then on. The opening is
what the battle's load posted (a mission script's hints, messages and
voice-overs of its load, announced on the battle's first step; never a kill).
A server's battle starts with no one in it (a rotation's players arrive after
their loading screen, seconds after it began), so each joiner is sent the
opening as of his arrival (its tick his join's, its end that much later), as
a local battle shows its player the opening at its start. A client turns a hint into a
message and a voice-over into a voice notice, the same notices a local world's
view makes of the script's own hints and voice-overs; the screens draw the
messages, and the battle's sound plays each voice-over once, for a local world's
battle and a server's alike.

**Roster** (kind 28, required). The `entity-registry` message of a scripted
battle carries `"records":[…]` with one reliable record beside the registry:
`{"teams":[{"name":text,"icons":[…],"classes":[…]},{…}]}` for teams 1 and 2, at
most 4 icons and 12 classes each, the classes a player may choose by their ids
in the registry of the same message, the default first. Every other battle's
message has no such key, and a client whose registry named no mission refuses a
mission's records. A restarted battle must have the roster its players were
admitted to.

A player of a scripted battle deploys as the local player of one does: from the
script's own population, never from his connection. His spawn edge (a class of
his team and a command post, or none) is a choice, refused while he lives or his
four seconds after a death last. The mission then gives him a body at a post of
his team, in the next spawn wave of a server's battle (one every 15 seconds) and
in his team's turn within it, if his class has room and his team a ticket. Until
then he may choose again, and after changing teams he must. A class whose limit
is reached cannot be chosen. A player who joins takes a bot's place with the wave
it waited for; a server that deploys joining players by itself gives them their
team's first class with room. So what the screens need to offer a deployment is
all replicated: the team's classes in order (the roster record) and those of them
the mission has room for now on either team (tag 6, plus the spawn summary's
classes on the player's team), the posts (the post
summaries, and the spawn summary's for those he may take), whether he may choose
(the spawn summary's phase), what he waits for and whether he has chosen (tag 4).

The admission counts a mission at the most its records carry: the match, timer
and class-availability fields, the personal, deployment and class fields of
every roster slot, all objective fields and all systems.

## Combat continuity: the hero checkpoint (kind 4, draft)

A hero's prediction needs his combat state at the tick of his correction:
the combo state machine and its clocks, the button event history it matches
transitions against, the hit lists that stop one swing from striking a
target twice, the hold timers, and a thrown lightsaber's flight. The combat
code's lossless layout, measured at its reachable maximum, is 1,421 bytes of
combo state plus 258 bytes of thrown lightsaber, 1,679 bytes when both are at
their largest, and 59 bytes empty: one part empty, two parts full.

So the checkpoint travels in its own datagrams beside the own correction
(`rust/sim/src/wire/checkpoint.rs`):

- Each part datagram is `BNK1`, the epoch (u16), the authority tick (u32),
  the subject (the unit the correction is of, u32) and its life (u32), the
  record kind (u8) and revision (u16), the part's index and count (u8 each)
  and the whole checkpoint's length (u16), then up to 1,040 bytes of it. At
  most four parts: 4,160 bytes. Every part fits a datagram.
- A checkpoint belongs to exactly one own correction: the same epoch, tick,
  subject and life, of the kind and revision the match's plan uses. The
  client commits the two together, whichever arrives last, and replays from
  them.
- Checkpoints only move forward, by epoch and then by authority tick. A part
  older than the newest accepted correction, or than the checkpoint being
  assembled, is stale and ignored; a whole checkpoint whose correction was
  superseded is never committed; a part of another unit or life at an
  accepted tick, or a second checkpoint for one tick, is refused. A lost part
  costs that tick's checkpoint, never a wrong one: the client goes on
  predicting from what it has, and the next tick's parts replace it.
- Reassembly is bounded: one checkpoint at a time, in storage reserved once.
- The checkpoint goes whole, not as a delta, so no part depends on an older
  one the client may not hold.
- A match whose table does not name kind 4 refuses a part; a client that
  skips an optional kind 4 ignores its parts at the revision the match
  advertises and refuses a part of any other.

Revision 1 of the record is the combat code's measured layout, keeping the
target life identity and absolute clocks its proof needs, read and checked
against the matched machine (state, transition, grace and attack-group
indices, finite clocks, deflect masks, target lives) before either the combo
or the throw is restored. The soldier body's bits 3 (combo) and 4 (thrown
saber) then say that a correction has a checkpoint, in the next revision of
kind 2.

The authority applies a hero's block and its other durable edges one an
application tick, in the order they were sent, within the life they were
sent for: a release then a press that reach one tick end pressed, a press
then a release is a tap of one tick, and every life starts with block
released, so a control still held from the previous life is pressed again in
the new one.

## Draft records

Each definition below is a draft: no build writes or requires it, and its
first revision may still change. Each is the body of its kind's carrier, after
the revision the carrier puts first. All fixed-width numbers are little endian. A
normalized u16 maps 0 to 65,535 onto [0, 1] with nearest rounding. Ticks are
u32 at 60 Hz and wrap. A world field's key is its schema and id, its
generation is the entity's life (or the epoch for per-match state), and
removal and the absolute and delta modes are the envelope's. Every keyframe
carries every active field, so a joining client gets the whole persistent
state in its first complete snapshot. Unused flag bits are zero. Every reader
checks finiteness, ranges and references to lives before anything is stored.

## Hero records (kinds 5 and 6)

Both are world fields keyed by the soldier's own id (100,000 plus the unit
slot) and life, written from the battle's own combat state (`rust/sim/src/wire/
{melee,thrown}.rs`) in Instant Action and online alike, and read by the same
readers before a 2005 match's receiver stores them. Each payload starts with
its revision (u16).

**Combat display, `0x0205`** (kind 5, revision 2, optional): `flags:u8` (bit 0
a melee pose, bit 1 a cast pose, bit 2 the cast held, bit 3 choked; a held cast
only with a cast), then the parts the flags name, in that order: the melee
pose (34 bytes: combo state, clock origin, loop, clip length, blade mask,
reaction, swing turn), the cast pose (13 bytes: the weapon class hash, fire
slot, clock origin) and the choke (4 bytes: the clip the holding power names).
3 to 54 bytes with the revision. Constant within a combo state and a cast, so
it is sent again only when a part changes.

**Thrown saber, `0x0206`** (kind 6, revision 1, optional): `flags:u8` (bit 0
pressed, bit 1 launched), the press tick, then the launch block a viewer
replays the flight from with the battle's own throw rule; 72 bytes with the
revision, the same from the press to the catch, so it is sent once per press
and once per launch.

**Roster points, `0x0207`** (kind 7): reserved; no build writes it yet.

## Damage feedback in a 2005 battle

The released 78-byte combat record (schema 3) is unchanged, except that in a
2005 battle bit 1 of its `u16` flags (byte 52) says the hit was critical, as
the world's own contact says it. The authority sets it after the released encoder
and check have run; a client reads and clears it before the released decoder
reads the rest. A 2004 battle never sets it.

## Space Assault records (revision 1)

Implemented for Space Assault battles (`rust/sim/src/wire/space.rs`,
`rust/bfe-client/src/online/space_receiver.rs`). All fixed-width numbers are
little endian, ticks wrap at 60 Hz, unused flag bits are zero, and every reader
checks finiteness, ranges, counts and trailing bytes before storing state.

**Pilot and gunner controls** (kind 16, revision 1, required in Space matches).
Carried in the 2005 input packet's records section (`flags & 2`), aligned by
sequence to the bundled `BNI1` movement commands: `count:u8` (1 to 8), then per
entry 26 bytes: `sequence:u32`, `flags:u16` (bit 0 `use_pressed`, 1
`takeoff_or_land`, 2 `boost`, 3 `eject_held`, 4 `fire_primary`, 5
`fire_secondary`, 6 `lock_target`; bits 7..8 `cycle_weapon`, bits 9..10
`trick_roll`, bits 11..12 `trick_flip`, bits 13..14 `terminal_cycle`, bit 15
reserved = 0), nine `i16` axes (`throttle`, `roll`, `stick_x`, `stick_y`,
`mouse_x`, `mouse_y`, `turret_look_x`, `turret_look_y`, `turret_turn`), and
`reserved:u16 = 0`. Every entry must name one of the packet's commands without
duplicates. While mounted in a starfighter or remote chair, the client transmits
at 60 Hz with one-packet redundancy of recent entries (~16 kB/s upstream on a
clean link, ~18 to 19 kB/s at 2% packet loss, compared with ~5.5 to 7 kB/s on
foot). The server parses the
records section into fixed stack storage with zero extra heap allocations and
feeds decoded `PilotControls` through the same shared Space pilot and terminal
seat driver (`rust/sim/src/space/local.rs`) connected to both local Instant
Action and network players.

**Starfighters, seats and cockpit state** (kind 17, revision 1, required in
Space matches). Two carriers:

- Shared world fields `0x0211`:
  - `id = 0..15` (`tag = 0`, 8 fighter slots per chunk): `rev:u16`, `tag:u8 = 0`,
    `chunk:u8`, `alive_mask:u8`, then per set bit 29 bytes (`generation:u16`,
    `profile_team:u8`, `flags:u8` [state, thrusting, boosting, mobile spawn,
    trick], `position:[u24; 3]` in map `Wide` codes, `velocity:[i16; 3]` at
    1/32 m/s, smallest-3 6-byte quaternion, `health:u16`, `bank:i8`, `gear:u8`).
    Unchanged parked chunks emit 0 bytes in delta.
  - `id = 256` (`tag = 1`, seat occupancy map): `rev:u16`, `tag:u8 = 1`,
    `count:u8` (at most 128), then 4 bytes per occupant (`unit_slot:u16`,
    `mount_kind_and_seat:u8`, `mount_index:u8`). While a soldier is carried
    inside a Space fighter, his `WireClass::Soldier` pose stays frozen to his
    last on-foot values so seated crew do not duplicate fighter motion.
- Own correction body `0x91` (`0x80 + 17`, inside `schema 11` `BNO1` whenever
  the local player holds a Space fighter seat or remote chair): `rev:u16`,
  `mode:u8` (0 pilot, 1 gunner, 2 chair), followed for modes 0 and 1 by full
  `f64` replay kinematics (`position`, `velocity`, `quaternion`, `airspeed`,
  angular rates, `bank`, `gear`, timers, `energy`, `health`, `stick`, the
  stick shaping the craft keeps between ticks (`roll_state`, `level_timer`,
  `yaw_buildup`, `pitch_buildup`), the pilot's double taps (the detector's
  `clock:f64`, a byte of directions: bits 0-1 the roll held at the last tick,
  2-3 the throttle held, 4-5 a roll tap that can still pair, 6-7 a throttle
  tap, each 0 none, 1 positive, 2 negative; then the time of each such tap, no
  later than the clock and at most 0.3 s before it), optional
  `trick_axis_right` and `gunner_angles`), seat weapons (up to 5 x 17 bytes),
  and active target lock; or for mode 2 by chair and turret building indices,
  `aim:[f64; 2]`, `feet:[f64; 3]`, turret angles, cooldown, heat, overheat and
  shot counter. On every correction, the client replays pending quantized
  `PilotControls` tick by tick on the server's 60 Hz schedule (`ack_tick + 1`
  through the newest stamped command, merging same-tick commands and repeating
  held controls across gaps) through the same flight, chair, and attitude
  extrapolation math, feeding the same cockpit builder used locally. Steady
  Space field validation, decoding, snapshot publication, and prediction run
  with zero heap allocations into pre-reserved buffers.

**World animation clock, `0x0213`** (kind 19, revision 1, required in Space
matches), id 0, life the epoch: `rev:u16`, `count:u8` (at most 32, the matched
map's animation groups in their order), then per group `state:u8` (0 stopped,
1 playing, 2 paused) and `origin:u32` (playing: the tick at which the group's
clip time was zero; paused: the elapsed ticks at the pause; stopped: 0). A
client derives every animated frigate and prop from the authority tick, with no
per-frigate transforms. Sent whole on any change.

**Space ordnance and bursts, `0x0214`** (kind 20, revision 1, required in Space
matches). Two shared world fields:

- `id = 0` (`tag = 0`, live guided, homing, bomb and timebomb rounds):
  `rev:u16`, `tag:u8 = 0`, `count:u8` (at most 44), then 19 bytes per round
  (`slot:u8`, `serial:u16`, `weapon_idx:u8`, `position:[u24; 3]`,
  `velocity:[i16; 3]` at 0.125 m/s).
- `id = 1` (`tag = 1`, launches, impacts and destruction bursts across the
  current and preceding 20 Hz publication windows, capped at 896 bytes):
  `rev:u16`, `tag:u8 = 1`, `launch_count:u8` (at most 32), `impact_count:u8`
  (at most 32), `death_count:u8` (at most 8), followed by 19-byte launches
  (`flags` bits 0..2 `tick_offset` 0..=5, bit 3 beam, bits 4..6 team, bit 7
  zero), 13- or 15-byte impacts (`flags` bits 0..2 `tick_offset` 0..=5, bit 3
  shield, bit 4 rigid, bit 5 normal present, bit 6 blast, bit 7 zero, with
  optional 2-byte octahedral surface normal), and 12-byte fighter/building
  destruction bursts. The authority samples `0x0214` only on 20 Hz publication
  ticks, keeping each event for two consecutive publications (ages 0..=5 ticks;
  fresh events take priority over carried-over ones under the cap), and the
  client deduplicates received events in fixed-size rings so a single dropped
  or skipped snapshot loses no shots, impacts, or explosions. A snapshot is
  complete only when all its parts and its own datagram arrive, but its parts
  are applied as they arrive: the events of a snapshot that never completes
  are kept with that snapshot's tick and drawn with the next complete one. An
  event is therefore lost only when the events part of both snapshots that
  carry it is lost (measured on a real server through a lossy link: none of
  7,024 launches lost at 0% loss, 0.03% at 2% and 0.95% at 10% datagram loss,
  none drawn twice).

**Space turrets and doors, `0x0215`** (kind 21, revision 1, required in Space
matches). Two shared world fields:

- `id = 0` (`tag = 0`, Space turret aims in deterministic building order):
  `rev:u16`, `tag:u8 = 0`, `count:u8` (at most 96), then per turret `yaw:i16`
  and `pitch:i16` scaled across `[-pi, pi]`.
- `id = 1` (`tag = 1`, interior proximity doors in deterministic door order):
  `rev:u16`, `tag:u8 = 1`, `count:u8` (at most 64), then per door `open:u8`
  (0..255) and `flags:u8` (bit 0 locked, bit 1 opening).

## Capacity, census and budgets

A 2005 battle reserves the wire's whole domain: 512 entity slots, 1,024
world fields, 512 known identities and 1,024 rows, and its snapshots may take
up to 256 parts, which is what every receiver reads. Both staging banks (the
stage and the capture stage that swaps with it) hold 1,024 fields, and the
publication arena holds four whole groups beyond the released arena. A 2004
battle keeps the released reservations.

Admission counts what the battle can field at once, with overflow-checked
sums, and refuses a battle that does not fit, naming its totals: two bodies for
every place on its rosters, the one that fills it and the one its last life
fell in (a player who joins takes a bot's place, so places bound the bodies);
placed and spawnable hulls; ground items (the mine and power-up pools); turrets
that hold their own slots; remote droids, one per body when a weapon deploys
one; and world fields (each body's unit and display fields, props, animations,
traps, tows, the common summaries of every command post and every player, and
the most a mission's records carry). It counts ids too: a capture keeps every
body, hull and target it has ever published, so those admitted, the hulls still
to be born and a body for every place must fit its 512, and the bodies a match
builds later, as players change class, stay below that.

Admission then sizes the battle's largest keyframe with the writer's own
record and field writers: every entity at its largest record (each hull with
the most seats any of its vehicle classes has, as many soldiers seated as
there are seats) and every producer's every field at its largest payload (a
unit field with the longest repair identity and every weapon of the
best-armed soldier class at its fullest, a soldier display of 11 bytes, each
JSON presentation record with every number at its longest form, names of 32
bytes, the records of players who have each beaten every player on both teams,
each checked against its writer). The writer fills parts in order and opens a
new part only when the next group does not fit, so a closed part holds at least
its room less that group: from the total and the largest groups admission
bounds the parts the keyframe can need in any order of its records, and refuses
a battle whose bound exceeds the 256 parts a snapshot may take. Every battle it
admits can publish a whole keyframe at any moment of it. The bound is far above
what a battle takes: nothing in one is at its largest everywhere at once.

For the generated battle below the bound is 25 parts at 64 fields and 52 at
286 (the writer used 13 and 30). With the summaries of 64 players and eight
posts its largest keyframe is 64,474 bytes in 68 parts and is bounded at 102; a
mission at the most its records carry adds 12,452 bytes: 81 parts, bounded at
122. In an invented space battle (58 soldiers, 30 two-seat fighters, 52 turret
slots, 144 space objects, the animation clock) with each draft 2005 field at
its largest size, the whole keyframe with all 512 guided ordnance slots is
52,288 bytes in 50 parts, bounded at 59. Reliable bursts travel on the reliable
lane and are budgeted by their own carrier, not by the keyframe.

A snapshot is complete only when all its parts arrived, so the parts a keyframe
takes are what a joining player waits for. Through the whole chain, with every
field changing and 100,000 bytes a second a peer (one run each), a joining player
completed his first snapshot over a path losing one datagram in 120 within 0.8
seconds at 64 parts and 3 seconds at 128, and over one losing one in 40 within 3
seconds at 64, 3.3 at 96 and 22 at 128; at 160 parts and more over that path he
did not within 50 seconds.

The receiver keeps 32,768 field versions for 128 completed baselines and 32
assemblies. When a valid stream fills the bank, it retires the oldest
completed baselines (never the four newest, the one being completed or the
current one) rather than stalling; a 2004 battle cannot fill it.

Measured with a battle generated from the protocol's limits and invented
distributions (37 hulls, nine of them four-seat, 101 soldiers with 52 seated,
29 items and 7 remotes in a box of about 6 by 3.2 by 6.3 km,
`rust/sim/tests/crowded`):

| World fields | Keyframe | Delta, every field changed |
| ---: | ---: | ---: |
| 64 | 13,262 bytes in 13 parts | 6,949 bytes in 7 parts |
| 286 | 30,876 bytes in 30 parts | 24,539 bytes in 24 parts |

One keyframe of that battle carries the whole 1,024-field domain in 256 parts
(319 fields in the released 32). A loss-free stream of 170 snapshots with a keyframe every 40
completes every snapshot at 64, 286 and 600 fields, with every field changing.
Through the whole chain (staged fields, capture, the retained ring, the
publisher at 100,000 bytes a second per peer, a path that loses one datagram
in 40 and delays one in 30 by two to five ticks, lives dying and returning)
over 30 seconds at 286 fields, the player present from the start completes
17.7 snapshots a second (longest gap 22 ticks) and one joining after 10
seconds 15.0 a second, its first within 2 seconds, each within the budget.
Staging costs 58 µs a tick and capture and publication 162 µs for two players
in a release build.

## Release

Protocol 4 and `bfe-sim-30` are released once, when the 2005 game is
switched on, as client and server built from the same revision. Protocol 3
clients and servers keep their own matches until updated; mixed versions
refuse each other with the existing version texts at every entry point.
Later 2005 records get their revisions as the code that produces each is
final, each behind its kind, with no protocol change.

### The gate

The 2005 game is switched on in one place: `bfe_family::SWBF2_AVAILABLE`
(`rust/bfe-family/src/lib.rs`). `bfe_family::Gate::BUILT` follows it, and
it is the only gate shipped code holds. A gate is a value, never a switch
for the process: each check of a battle's game takes the gate of whoever
started that work. A server takes `ServerConfig::gate`, which only code that
starts a server sets; `config.json` and the command line cannot. The server
checks it in its rotation and content checks, its battle load and its restart,
before any content is read. The interface (the shell's picker, the battle
screens) and Instant Action ask `GameFamily::is_available`, which is
`Gate::BUILT`.

Two pieces of code open the 2005 game, and neither is in an ordinary build.

- `bfe_test_gate::OPEN`, in the crate `rust/bfe-test-gate`, which only
  `[dev-dependencies]` name. A test hands it to what it starts; other tests in
  the same binary keep the closed gate.
- The developer switch for online play (`rust/bfe-family/src/dev_online.rs`),
  compiled only with `--cfg bfe_dev_online`:
  `RUSTFLAGS="--cfg bfe_dev_online" cargo build --release -p bfe-server -p bfe-client`.
  In such a build `bfeserver` and a client's online join also play the 2005
  game while `BFE_DEV_SWBF2_ONLINE=1` is in their environment. It is read at
  startup, is never saved, and without it both hold the built gate. No
  feature, manifest, build script or workflow sets the cfg, so a build with
  every feature on has no switch either, and the variable does nothing there.

The checks that keep it so look for each one's marker text, which any binary
containing it carries. `build/ci/check-release-gate.sh`, which the Linux CI job
runs, fails when any workspace crate depends on `bfe-test-gate` as a normal or
build dependency for any target, with every feature of every workspace crate on
(the graph `cargo build --workspace --all-features` resolves); when a release
binary that does use either lacks its marker, which would make the search prove
nothing; when an ordinary release binary has the switch or follows it; and when
the server CI builds contains either marker. Every release build stages its
client and server through `build/native/stage.py` (Linux x86_64 and ARM64,
Windows, macOS), which refuses a binary that contains either marker.

The version source is `rust/sim/schema/transport-layout.json`. Change it there,
then rebuild the derived files with `rust/xtask/`; `build/ci/check-generated.sh`
verifies them.
