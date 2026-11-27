# Snapshot and delta encoding

The authority steps at 60 Hz and normally captures every third tick. The same
publisher runs for Instant Action and a dedicated server. The transport carries
`BNS1` entity and world datagrams and `BNO1` own-player datagrams on its state
lane. Each datagram is at most 1,100 bytes. All fixed-width integers are little
endian.

## Position bounds

Every Battlefront (2004) map uses X and Z from -1024 m to 1024 m and Y from
-400 m to 300 m. The codec takes the resolved battle family and the original
map metadata at admission. A Battlefront II map supplies the unpadded playable extents in a top-level `map.json` field
`wireBounds`, with `min` and `max` arrays in world XYZ metres. Preparation takes
the union of the playable bounds across the map's supported layers, including
flight limits, reachable geometry and spawn locations. Admission rounds each
minimum down and maximum up to whole metres and adds a provisional 64 m at
each face. The importer writes the unpadded extents. Before enabling a map,
measure how far replicated entities travel past each face during a flight-limit
exit and return, knockback, or debris travel until retirement. Size the margin
from the greatest supported excursion, including travel between captures.
Admission requires finite, strictly ordered source coordinates, padded faces within
±1,048,576 m and padded spans of at most 65,536 m. A 2004 map always uses the
fixed box, regardless of this field.

The codec implements per-map boxes and wide positions. Every 2004 battle uses
the fixed 2004 box; a 2005 battle uses its map's box. A protocol 3 client
ignores position flag bit 1 and reads three u16 codes from a wide absolute
group, so it would desynchronise on that datagram; protocol 4 admission
refuses it with the version text first.

Capture, the cosmetic journal, the client decoder and item prediction take
the same immutable box. For Battlefront II the codec derives it from the
resolved family and the prepared map. The gameplay fingerprint includes its
`map.json`. When any
padded axis is wider than the corresponding 2004 axis, all three axes use
24-bit absolute codes. Otherwise they use 16-bit codes.

| Field | Encoding |
|---|---|
| Position | Code = `clamp(round((position - min) / extent * maxCode), 0, maxCode)`, rounding midpoint ties up. Decode as `min + code / maxCode * extent`. The maximum code is 65,535 for u16 and 16,777,215 for u24. In the fixed box a step is about 31.25 mm in X/Z and 10.68 mm in Y. |
| Position delta | Group flag bit 0 selects absolute codes. Otherwise three signed i8 differences in [-127, 127] add to the matching baseline codes. Bit 1 selects wide codes and is set for both absolute and delta positions on a wide map. Bit 7 marks discontinuity independently. |
| Velocity | A width byte followed by three signed i8 codes at 0.25 m/s, or three signed i16 codes at 0.03125 m/s. Items always use the latter. Other bodies use it when a component exceeds 31.75 m/s in magnitude. |
| Body aim | Unsigned yaw and signed pitch at 65536 steps per turn. Turrets and mount angles instead use signed codes with 32767 steps per half turn. |
| Hull orientation | Smallest-three quaternion in six bytes. The omitted component's index uses two bits and the three retained components each use 15 bits. |
| Health and status | Health uses integer u16 units. Other status fields depend on the entity class, for example soldier posture and charge share a byte. |

For a 16 km axis, u16 would step by about 244.14 mm. A u24 step there is about
0.954 mm with at most 0.477 mm of rounding error. Even at the 65,536 m span
limit a u24 code has less than 1.954 mm of in-range rounding error. Both faces
of the box are represented exactly. Item prediction uses half a code step on
each axis plus its existing 2 mm tolerance.

Position saturation is only an encoding limit. It does not constrain the world
simulation. Turret transforms are authored, so their entity records carry aim
and status groups without a position group. The own correction carries replay
state in its own schema. It does not use the entity position codes. Compact bolt
launches and impacts use the map's position codes when the origin fits the box
and the launch offset fits its signed fields. The tags are 1 (launch) and 2
(impact) for u16, or 3 (launch) and 4 (impact) for u24. Other events and
out-of-box origins keep their raw payloads under tag 0.

## Baselines and records

The host retains 128 snapshot ticks per battle. Recipients acknowledge a
complete snapshot by epoch and tick in input or a standalone keepalive. The
publisher groups clients with compatible held baselines and encodes each
group's shared bytes once. Baseline tick 0 denotes a keyframe. An unavailable
baseline makes the host select another held baseline or send a keyframe.

Each shared part starts with a world-field section. Fields are keyed by schema
and identity, with a generation and either an absolute payload, a removal, or a
bitmap of changed eight-byte words when the delta saves space. Entity records
follow in ascending slot order. The record byte has a two-bit operation
(update, new identity, removal) and a six-bit mask of class-specific field
groups. An identity change writes the complete record. A change may omit
groups that match the baseline. Mounted riders and their hull pack into one
part when their records change together. The part counters use the compact
header for up to 15 parts and the extended counters for larger snapshots.

An own datagram can carry the player's correction, private world fields and a
length-prefixed cosmetic bundle. Cosmetics may occupy a second own datagram.
Damage and death feedback use the reliable events lane. The receiver keeps
integer position codes in its baselines and widens them to metres when
publishing records and pose samples. It applies each part to its named
baseline, waits for the required parts before acknowledging the tick and treats
a missing baseline as recoverable loss.
