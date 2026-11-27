# Classic AI: navigation

Runtime ownership is described in [Native AI ownership](sim-ai.md). Bot policy
runs in `rust/sim/src/ai/`. Function names are the Rust names;
object-shaped examples document the behavioral contract.


Navigation for the Classic AI (`docs/ai.md`): the extracted planning data
(`tools/zero_ai.py` → `public/maps/<map>/ai-nav.json` + `ai-nav.bin`) and the
runtime under `rust/sim/src/ai/nav/`. Behavior follows the 2004 game's route
planning and movement; this file records the schema, the module APIs, what is
provisional, and findings made while implementing.

## `ai-nav.json` schema 1

Written by `python3 tools/zero_ai.py <map>`; gitignored like everything under
`public/`. All arrays keep source order. Coordinates are the archive's own (the
same frame `map.json` uses); nothing is flipped or converted by the extractor.
Transforms are the raw 12-float `XFRM` (three basis rows, then the
translation: `[Xx,Xy,Xz, Yx,Yy,Yz, Zx,Zy,Zz, Tx,Ty,Tz]`; Z is a hint's
forward). Hashes are lowercase 8-digit hex of the repository's FNV-1a with
each byte OR 0x20 (`tools/zero_hash.py`).

```text
{
  schema: 1,
  generator: "tools/zero_ai.py 1",
  map: "nab2",
  source: { path: "NAB/nab2.lvl", bytes: 42728724, sha256: "…" },
  world: "naboo2",              // NAME of the (first) wrld chunk, or null
  worlds: ["…", "…"],           // only when the archive has more than one wrld (concatenated in order)
  counts: { … },                // summary, below
  graph: { … } | null,          // the `plan` chunk; null when the archive has none
  hints: [ … ],
  barriers: [ … ],
  boundary: { … },
  paths: [ … ],
  regions: [ … ],
  warnings: [ … ],
  bin: { file: "ai-nav.bin", bytes: 147920, sha256: "…" } | null,
}
```

### `graph`

```text
graph: {
  info: [hubCount, arcCount, third],     // INFO: i16, i16, u16 as stored (third is 5 in stock data)
  hubs: [{
    name: "Hub0",                         // char[16], up to the initial NUL (latin-1)
    position: [x, y, z],
    radius: r,
    arcs: [a0, …, a7],                    // incident arc indices; 255 = unused slot
  }],
  arcs: [{
    name: "Connection1",                  // char[16]
    hubs: [first, second],                // hub indices in stored order
    mask: 0x0f,                           // u32 traversal mask
  }],
  routes: {                               // the packed routing words, in ai-nav.bin
    file: "ai-nav.bin", byteOffset: 0,
    count: 5 * N * N,                     // u32 little-endian words
    tables: 5,
    order: "hub,table,destination",       // word index = (hub * 5 + table) * N + destination
  },
}
```

`ai-nav.bin` is exactly the five `u32[N]` routing arrays of every `NODE`
record, concatenated in hub order (the file's own byte order). A word packs up
to four candidates, least significant byte first: slot = `byte & 7` (an index
into the hub's `arcs`), weight = `(byte >> 3) & 31`. Zero means no route.

The runtime hub capability mask is not stored: like the original game's
loader, the runtime ORs the masks of the arcs in the hub's eight slots.

### `hints`

```text
hints: [{
  type: "2",                              // INFO/TYPE string as stored
  kind: "cover",                          // snipe | patrol | cover | jetjump | minefield | land | null
  bit: 4,                                 // internal type bit (1,2,4,8,16,32), 0 when unsupported
  name: "hintnode0",                      // INFO/NAME
  xfrm: [12 floats],                      // INFO/XFRM, raw
  properties: [[hash, name, value], …],   // PROP children in order: "bf0f1cd1", "PrimaryStance" | null, "2"
}]
```

`kind` comes from the FNV hash of the TYPE string, as the original game's
loader compares it: "0" snipe, "1" patrol, "2" cover, "4" jetjump, "5"
minefield, "6" land. Anything else (stock data has no "3") is kept with
`kind: null` and a warning; the runtime drops it as the loader does. Known
property names: PrimaryStance, SecondaryStance, Mode, Radius, CommandPost.
Values stay raw strings; the runtime parses them (sscanf semantics, including
the unparseable-value quirks).

The extractor does not expand radius hints. The runtime builds the loaded
hint list from these records (`buildHints`), including the BaseHint pool's
capacity checks and the four-copy radius expansion.

### `barriers`

```text
barriers: [{
  name: "barrier12",                      // INFO/NAME
  xfrm: [12 floats],                      // INFO/XFRM, raw (translation z NOT negated)
  size: [sx, sy, sz],                     // INFO/SIZE; x/z are footprint half extents
  flags: 0x3f,                            // INFO/FLAG u32 traversal mask
  properties: [[hash, name, value], …],   // BARR-level PROP children (the loader reads and ignores them)
}]
```

The original game's barrier loader negates the translation's z and keeps the
basis rows as stored. The runtime applies that adapter (`barrier_from_record`);
the extractor writes the file's values.

### `boundary`

```text
boundary: {
  configs: [{                             // every top-level bnd_ chunk
    name: "64da3d0a",                     // NAME u32
    entries: [{                           // DATA entries, SCOP children attached to the DATA before them
      key: "54eacca7", keyName: "Boundary" | null,
      args: ["54eacca7", …],              // raw u32 arguments, hex
      strings: "…hex…" | null,            // string table bytes when its size is nonzero
      children: [ … ],                    // same shape
    }],
  }],
  references: [{ hash: "54eacca7", path: 3 | null }],  // Path children of Boundary entries, in order;
                                                       // path = index into `paths`, null when missing;
                                                       // hash null for a Path entry with no argument
}
```

The runtime builds one allowed-area polygon per resolved reference (points
x/z in path order) and applies the half-plane containment test. Missing
references are skipped, as in the original game. No `bnd_` means no boundary
enforcement (every point is inside).

### `paths`

```text
paths: [{
  name: "CP1Spawn",
  info: [count, word2, word3],            // i16, i16, u16 as stored; count must equal the points
  points: [[x, y, z, qw, qx, qy, qz], …], // PNTS: position then quaternion w,x,y,z
  prop: "6e756d…" | null,                 // raw PROP bytes, hex (not hash/string pairs; unused at runtime)
}]
```

### `regions`

```text
regions: [{
  type: "box", name: "cp1_capture",
  xfrm: [12 floats], size: [sx, sy, sz],
  properties: [[hash, name, value], …],   // optional hashed PROP children
}]
```

Duplicate names stay separate entries.

### `counts`

```text
counts: {
  hubs, arcs, routeWords, routeNonzero,
  weightSums: { "31": n, "30": n, … },    // nonzero words by candidate weight sum
  hints: { cover: n, jetjump: n, … , unsupported: n },
  barriers, paths, pathPoints, regions,
}
```

### `warnings`

Semantic findings that do not stop extraction. Each is
`{ kind, message, …fields }`:

| kind | Meaning |
| --- | --- |
| `routeCapability` | A packed candidate's arc lacks the table's capability bit (`hub, table, destination, slot, arc, arcMask, word`), once per word and slot. Shipped data has ten (END/end1 4, KAS/kas2 3, KAS/kas2_h 3); they are preserved. |
| `routeWeightSum` | Nonzero word whose weights do not sum to 30 or 31 (`hub, table, destination, word, sum`). |
| `routeHang` | A word the original game's arc selector could never finish (`…, word, reason`): `zeroWeights`, or `singleSlot` (every candidate on one slot, first weight below 31, which loops when that slot is the previous arc). The runtime guards these. |
| `hubSlotGap` | A hub lists an arc after an unused (255) slot (`hub, arcs`); the alternate-arc fallback stops counting at the initial unused slot. |
| `hubMaskDiffers` | The OR over the hub's slots differs from the OR over every arc touching the hub (`hub, slotMask, touchingMask`). |
| `zeroLengthArc` | Coincident arc endpoints (`arc, hubs`); the corridor test would divide by zero, so the runtime skips these arcs. |
| `hintType` | Unsupported hint TYPE (`hint, name, type`), dropped at load. |
| `jetForward` | A jet hint forward vector that is not unit length (`hint, name, length`), kept as stored. |
| `barrierBasis` | A barrier basis that is not orthonormal (`barrier, name, xLength, zLength, dot`), kept as stored. |
| `boundaryPath` | A boundary Path reference with no matching PATH (`config, hash`). |
| `duplicatePath` | Two PATH entries hash to the same name (`hash, paths: [first, later]`); lookups take the earlier entry. |

Structural problems are fatal (the tool exits non-zero and writes nothing):
INFO not 6 bytes, `NODE != N * (40 + 20N)`, `ARCS != 22A`, endpoint or slot
indices out of range, a slot's arc not touching its hub, a packed candidate
naming an unused slot, non-finite floats, chunk sizes that overrun their
container, a hint PROP before its INFO, XFRM not 48 bytes, SIZE not 12, FLAG
not 4, PROP not hash + NUL-terminated string, PNTS not a multiple of 28 or not
`count` points, negative counts, bnd_ DATA arguments or string tables that
do not fill the entry exactly, a missing or repeated required child, a SCOP
that does not follow a DATA, and nonzero padding. Unknown child tags are
framing-checked and skipped. In stock data region PROPs (only `Layer`,
07a640f6) sit at the `regn` level after INFO; no stock barrier has a PROP and
no map archive uses `lvl_` wrappers.

The tool: `python3 tools/zero_ai.py <map> [<map> ...] [--out DIR] [--archive
PATH]`; a fatal error prints `<map>: <chunk path>: <problem>`, writes nothing
for that map and exits 1. For other tools and tests: `decode_archive(data,
source_path, map_name=None) -> (doc, bin)` and `export_map(name, archive=None,
out=None) -> doc` (atomic writes; removes a stale `ai-nav.bin` when a map has
no plan).

### Stock data (five maps)

| map | hubs | arcs | routed words | weight sums | hints (loaded) | barriers | paths (points) | regions | boundary | warnings |
| --- | ---: | ---: | ---: | --- | --- | ---: | --- | ---: | --- | --- |
| nab2 | 86 | 137 | 10282 | 31: 10259, 30: 23 | cover 87, jetjump 5 (101 after radius rings) | 374 | 8 (63) | 16 | Boundary | barrierBasis 1 |
| bes2 | 119 | 138 | 70210 | 31: 70206, 30: 4 | snipe 19, cover 11, minefield 1 (31) | 514 | 7 (50) | 33 | boundary1 | none |
| kam1 | 88 | 97 | 38280 | 31: 38270, 30: 10 | cover 46, jetjump 11, land 8 (65) | 301 | 8 (73) | 49 | boundary1 | none |
| geo1 | 51 | 76 | 4632 | 31: 4632 | snipe 15, patrol 4, cover 49, minefield 10, land 7 (85) | 214 | 13 (123) | 19 | boundary1 | none |
| hot1 | 46 | 64 | 2042 | 31: 2040, 30: 2 | snipe 38, cover 3 (41) | 179 | 8 (36) | 43 | boundary1 | none |

Over all 22 retail archives (including `_h` variants and Tat3) the only
routeCapability occurrences are the ten listed above; the other warnings found
are nab2/nab2_h barrier 341's basis (X·Z = −0.0039) and yav2/yav2_h's two
paths named "Jungle1".

## Runtime modules (`rust/sim/src/ai/nav/`)

| File | What |
| --- | --- |
| `mod.rs` | `Nav` (the navigation service), `PlannerOptions`, the planner queue and service, every unit's `NavControl` |
| `data.rs` | `load_nav` (schema 1); the routing words are `bfe_content::content::route_words` |
| `graph.rs` | `NavGraph`: attachment, weighted next-arc selection, alternate arc; `contains_hub`, `contains_arc`, `corridor_gap`, `other_end` |
| `cursor.rs` | `GraphCursor`: one graph route (initialization branch tree, advance, shell waypoints, blocked-waypoint retry) |
| `barriers.rs` | `barrier_from_record`, `BarrierSet` (grid broadphase and the planner queries), `inside`, `corner`, `crossing`, `segment`, `ejection`, `detour` |
| `search.rs` | `BarrierSearch` (incremental best-first corner search), `OpenHeap` |
| `planner.rs` | the shared path pools: `AIPath`, `Pool`, `PathStore` |
| `executor.rs` | `PathExecutor`, `lookahead`, `near_radius`, `pass_radius`, `flyer_circle`, the executor thresholds |
| `control.rs` | `NavControl` (a unit's path and request state), `MovementRequest` |
| `goto.rs` | `GroundGoto`, `FlyGoto` (the ground and flyer movement helpers), `Nav::navigation_input` |
| `analogs.rs` | `analogs`, `along_path`, `coefficients`, `apply_jet_and_posture` |
| `hints.rs` | `build_hints`, `HintSet`, `pick_stance`, `pick_secondary`, `hint_bit`, `stance` |
| `jet_hints.rs` | the jet-hint helper ([ai-classes.md](ai-classes.md)) |
| `boundary.rs` | `Boundary` |
| `units.rs` | `kind`, `SizeType`, `traversal_mask`, `traversal_radius`, `nominal_speed`, `DEFAULT_RADIUS` |
| `math.rs` | shared numeric helpers and the plain point type |
| `tests.rs` | focused checks against recorded answers on the same inputs |

### `Nav`

```text
Nav::new(json, bin, posts, hint_capacity, pools)
// Nav::empty(pools) for a map without ai-nav.json.
// posts: the hint CommandPost names and the Land-hint association; optional.
// pools: { paths: 900, path_nodes: 7200, requests: 75, search_nodes: 1024, heap: 512 }
nav.graph          // NavGraph or none
nav.barriers       // BarrierSet
nav.hints          // HintSet (the loaded list: radius rings expanded, pool rules applied)
nav.boundary       // Boundary
nav.paths          // [{ index, name, info, points: [[x,y,z,qw,qx,qy,qz], ...] }]
nav.regions        // the ai-nav.json records, untouched
nav.warnings       // loader warnings; the graph's own reports are GraphReport
                   //   (ZeroLengthArc, RouteHang, RouteTable) in nav.graph.reports
nav.service(budget)            // one planner service
nav.idle()                     // no search running and nothing queued
nav.new_control()              // a unit's NavControl, created on first use
nav.outside(point)             // the boundary leaves the point outside
nav.path_by_name(name)
nav.query_hints(mask, center, half_extent, cap, requester, now)
nav.hint_available(hint, now) / nav.mark_hint(hint, owner, now)
nav.point_clear(p, r, mask) / nav.clip_segment(s, e, r, mask, normal)
```

The control core reaches these through its world (`control/world.rs`):
planner idle and service, `point_clear`, `clip_segment` (barrier
obstruction, shortening the end), `traversal_radius`, `traversal_mask`,
hint queries, availability and marking, `analogs` and `navigation_input`.

Scheduler (Controller): per AI cycle, `if !nav.idle() { nav.service(1) }
else { los_budget *= 2 }` then, if the deferred-LOS service returned false,
`nav.service(1)` again. The budget is planner barrier tests (10 per 1.0), not
paths or units.

### Movement helper (docs/ai.md interface)

`GroundGoto` and `FlyGoto` (`goto.rs`) hold a helper's state; the Nav
service's `ground_goto`, `ground_tick`, `ground_follow`, `fly_goto`,
`fly_tick` and the rest run it, and the control core reaches them through
`Core::helper_goto` and its siblings.

```text
// one helper per controller; it drives the controller's unit
helper.goto(point)                     // "route": graph route over 50 m, else one local path
helper.goto(point, { mode: "direct" }) // state 3 for Soldier/Droid (cut 1 m before a barrier,
                                       //   five retries), the state-4 corner detour otherwise
helper.goto(point, { mode: "straight" })  // state 8: a raw two-point path
helper.follow(ref, distance, remembered)  // state 5 (Soldier/Droid): ref = { unit, generation }
helper.stop()
helper.tick(dt)            // the helper FSM; writes controller.request
helper.arrived             // per state (see goto.rs)
helper.status              // executor status: 0, 1 no improvement for 5 s, 2 stationary 3 s (Droideka 6/6);
                           //   also 1 for a soldier or droideka stranded 5 s (6 s) without a path (below)
helper.failed              // the planner answered "no path" for the current goto
helper.stopped             // state 7
helper.state               // 1..8
helper.destination         // Vector3, after correction out of barriers
helper.destination_point(out, current)
helper.stance(0 | 1 | 2)   // stand / crouch / prone, applied by navigation_input
helper.jet(vector)         // jet request; zero = the soldier's own launch direction
helper.aim_at(point) / aim_at_target(ref) / aim_along_path()   // controller.aim_state 1 / 0 / 2
                           //   (controller.look_at / aim_at / look_along_path when present)
```

`goto` replans and stops the unit until the initial local path arrives, as the
original game's does: call it when the destination changes, not every tick.

The helper's world (`GotoWorld`) supplies the AI generator A, the clock,
weak-reference resolution (for follow), the second generator B (see
Findings), a line-of-sight or collision cast (jet clearance), and the
remembered point for `follow(..., remembered = true)` from Combat's memory.

### AI input step

```text
// Tick order step 2, every AI unit, every simulation step h:
nav.navigation_input(control, unit, h, aim_point, facing)   // returns the unit's Intent
```

It runs the unit's path executor, writes `intent.forward/right/turn/pitch`
from `controller.request` and the aim point, then applies a pending jet request
(jump, then jump again in the jump state, which lights the jetpack;
`intent.jetDirection` for a nonzero request vector) or the requested posture as
`intent.crouch/prone` toggles. `aim_point`: Combat's target-plus-lead point for
aimState 0, `controller.aim_point` for aimState 1, `null` for aimState 2 (30 m
along the path, or straight ahead near the end). `analogs(unit, request,
aim_point, dt, out, { executor, facing })` alone does just the conversion; pass
the unit's executor (or null) so the end-of-path slowdown is right.

Coefficients: forward MaxSpeed, strafe MaxStrafeSpeed, yaw MaxTurnSpeed and
pitch MaxPitchSpeed, the port's `classDef` fields of the same names (falling
back to the Soldier's `maxSpeed`, `maxStrafe`, `maxTurn`, `maxPitch`). A
droideka's strafe coefficient is 0, as the original game passes it for that
class: it walks forward only, as far as it faces its steering target, and
turns to face that target. (Its strafe keys turn it; given the class's
MaxStrafeSpeed, a droideka bot strafed toward its route while turning toward
an enemy to the other side, the two cancelled, and it walked back and forth
on the spot.) Signs:
the original game's strafe and yaw analogs map straight onto the port's
`right` (moves along (−cos yaw, 0, sin yaw)) and `turn` (positive decreases
aimYaw), and its pitch analog onto `pitch` (positive raises the aim).
Derivation: the transform's first row is up × forward = (cos yaw, 0, −sin yaw),
and the conversion's b = −(F × d).y must move the unit toward its target; tests
drive a kinematic soldier with the port's axes to its goal.

### Other helpers

- `NavControl`: `request`, `executor`, `segments`, `jet { pending, vector }`,
  `jetting`, `stance`, `avoid_side`, with the Nav service's
  `control_attach(unit)`, `control_status`, `control_publish(path, ready)`,
  `control_consume(mode)`, `control_install(path)`, `control_append(path)`,
  `control_stop()` and `control_update(dt)`. One per controller, kept across
  lives; `control_attach` resets it for a new unit or generation.
- `HintSet::query(mask, center, half, cap, owner, now)`: the generic box
  query (mask of hint bits; with an owner, hints reserved under 4 s ago are
  skipped; load order). `available(hint, now)`, `reserve(hint, owner, now)`
  (a hint and its ring). A HintSet holds its hints in `list`.
  Hint fields: `kind, bit, name, position, right, up, forward` (as authored),
  `primary_stance` (stance bit mask: 1 stand, 2 crouch, 4 prone),
  `secondary_stance`, `secondary_choices` (two-bit side mask), `mode`,
  `radius`, `radius_enabled`, `register_radius`, `command_post`,
  `command_post_name`, `reserved_at`, `reserved_by`, `copy` (0..3 for ring
  copies), `source`, `index`; ring links `link` = `next` and `siblings`
  (the ring); aliases `primary_mask`, `secondary_mask`, `side_mask`.
  `pick_stance(mask, random)` redraws (0 to 2) until the drawn stance is in the
  mask (stand for an empty one); `pick_secondary(choices, random)` does the same
  over the two-bit choice mask (-1 for an empty one).
- `traversal_mask(unit)`: `nav_mask`, else `classDef.AISizeType` (string,
  case-insensitive; unknown → Soldier), else 1. `traversal_radius(unit)`:
  `nav_radius`, else a quarter of the horizontal diagonal of
  `classDef.bounds` ([[min], [max]], the model's INFO box; half for Hover),
  else 0.476.

## Integration (outside `rust/sim/src/ai/nav/`)

1. `tools/extract.py`, end of `extract_map` (after `map.json`):
   `from zero_ai import export_map` and `export_map(name)`. A `ValueError`
   means malformed planning data; report it rather than abort the map.
2. Soldier contact avoidance (`rust/sim/src/movement/soldier.rs`, and the
   same in `rust/sim/src/units/droideka.rs`): where a polygon contact is
   resolved and its normal's |y| is under 0.3, the contact normal (pointing
   from the geometry toward the soldier, unweighted) is added to
   `avoidance`. Terrain contacts (category 64) do not count; soldiers are
   category 2 and do not count either. Static buildings and props (category
   8) and bodies (vehicles 4, live props 8) do. It is not cleared per tick:
   the path executor consumes and zeroes it.
3. Jet direction (`rust/sim (units) blankIntent`, the soldier's jet start):
   `intent.jetDirection` (Vector3 or null). For a non-player soldier a
   nonzero `jetDirection` replaces the launch direction literally (not
   normalized); null keeps the ordinary stick direction.
4. Class data for units: `bounds` (model INFO box, `zero_model.BOUNDS[GeometryName].box`)
   and `AISizeType` when authored (Combat's class extraction has it) on
   `classDef`, or set `unit.navRadius` / `unit.navMask` directly.
5. Generator B, the game generator (see Findings), is the battle's second
   stream, which the native owner registers at setup.

## Tests

- `cargo test` in `rust/sim`: `rust/sim/src/ai/nav/tests.rs` checks the AI
  generator's draws, open-heap ties, a barrier search and graph attach and
  selection against the reference answers on the same inputs.
- `python3 -m unittest tests.test_ai_nav_export`: synthetic chunk archives for
  every fatal and warning case, plus nab2/bes2 retail counts when the install
  is present.

## Provisional

The class and vehicle integration supplies model bounds/AISizeType, the second
generator, vehicle family steering and flyer landing, and authored jet-hint
jobs. See `ai-vehicles.md` and `ai-classes.md` for their validation and
remaining limits.

| File | Behavior | Why |
| --- | --- | --- |
| `barriers.rs` | Broadphase candidates in source order, each once, by bounding circle (radius: the length of SIZE) against the query box | The original game's grid visiting order and duplicate visits are not reproduced; only nearest-hit ties and work counts can differ |
| `analogs.rs` | The AI input's posture/jet timer (a Controllable field in the original game) is read as `soldier.lock > 0` | The field's port equivalent is not confirmed |
| `hints.rs` | Box query returns hints in load order | The original game's spatial order is unknown; only matters when `cap` truncates |
| `cursor.rs` | No compatible arc anywhere: the cursor goes straight to the exact destination | The original game has no defined behavior there; not needed for stock data |
| `goto.rs` | `follow(..., remembered)` needs the helper world's `remembered(target)` from Combat's memory | The remembered point lives in perception, which has to expose it |
| `zero_ai.py` | Duplicate PATH names resolve to the opening position in source order | The original game's path list order is unknown; only yav2 has duplicates |

Safe-port divergences (unsupported input only; stock data never hits them):
a routing word the selector could never finish returns no arc and reports
`RouteHang`; a mask with no routing table (bit 32 and up) is refused
(`RouteTable`); a full open heap refuses the insert and counts an overflow
(the original game writes past its 512 entries); a detour side with no corner
is skipped (the original game recurses on an uninitialized point); FSM
transition drains stop after 16 passes; empty paths complete at once.

Port changes against bots that stop moving (found with
historical headless measurements, see docs/ai.md "Stuck bots"). Each departs from
the original game where its behavior, or the port's arithmetic, left a bot
standing for good:

- **Stranded routes** (`control.rs`, `goto.rs`). When the helper gives up
  short of its destination (the planner answered no path, the graph route
  ran out of arcs or lost its waypoint, or a follow's direct path ran out
  more than 2 m from a target that keeps still), `NavControl::stranded` is
  set; with no executor the control then reports status 1 once the
  executor's own no-progress time has passed (5 s, 6 s for a Droideka), for
  soldiers and droidekas only. Stagnation recovery (priority 8) takes it from
  there. The original game reports 0 without an executor, so a bot whose route
  failed stood where it was. New orders (`goto`, `stop`, a new `follow`,
  `install_path`) and any installed path clear it; stranded again within 2 m
  of where it was, the time already spent there counts on (a fighter's look
  round between two failed routes from one barrier pocket restarted the
  count each time, and it never reached 5 s). The count starts over once
  the unit is more than 2 m away, and with each life.
- **A full request queue** answers the request as a completed failure
  (`goto.rs`); the original game's refusal left the helper waiting in state 2.
- **Starved requests** (`nav/mod.rs` planner queue): a queued request passed over while
  `STARVED` (20) later searches finished is taken next, oldest first, if its
  priority is one the queue serves (10 or less). Strict priority let a
  stream of priority-0 asks (fighters repositioning) hold a route's first
  leg (priority 1) back for as long as a fight lasted.
- **Droideka start** (`analogs.rs`): the 0.2 m stop applies only when the
  point it steers for is the waypoint itself. At rest its lookahead lies
  exactly 0.2 m on, where float rounding alone decided between moving and
  never moving.
- A dead or unbound bot's path work leaves the planner with it
  (the controller's unbind resets the control); before, its search and
  queued request went on taking the budget until its next life.

## Findings

- **Barrier frame.** The original game's barrier loader negates the
  translation's z, and that is right for the extracted data: with z negated,
  108/374 nab2 barrier centers sit within 2 m of a world instance (26
  without), and the mean distance to the nearest hub drops on every map (bes2
  78 m → 15 m, kam1 304 m → 14 m), while hints line up only without negation.
  Every retail barrier basis is flat, unit length and mirrored (determinant
  −1). In the runtime frame the corner order C+U+V, C+U−V, C−U−V, C−U+V
  therefore runs counter-clockwise, so the ejection nudge (Ez, 0, −Ex) points
  out of the barrier and the crossing normal (−Ez, 0, Ex) points into it
  (along the travel direction at the crossed edge). Hint transforms are proper
  rotations.
- **Two generators.** Route picks, hub shells and the AI's `int`/`range` draws
  use the AI generator. The GroundGoto jet attempt draws from a second,
  separate generator state (generator B, Integration 5).
- **Shell direction draw order.** A hub-shell direction draws three uniforms;
  the leading value becomes z, the next y, the last x, each `2 × (U − 0.5)`.
- **Search work counts depend on the broadphase.** A grid that culls far
  barriers counts fewer work units than feeding every barrier to the narrow
  phase (5 instead of 7 in the obstacle case) with the same path.
- **Entity types.** The direct goto and follow states are for Soldier or
  Droid; the jet attempt is Soldier only; Droideka stops under 0.2 m; Walker
  stops inside the near radius without a path.
- **Jetpack test.** The jet attempt's "has a jetpack" test is the class's
  JetFuelRechargeRate above zero; its fuel check uses JetFuelMinBorder.
- **Traversal radius.** A quarter of the horizontal diagonal of the class
  geometry's bounding box (half for Hover, and for a Walker under one
  team/mode condition); 0.476 m for rep_inf_trooper's box.
- **BaseHint pool** capacity is 350. Hint TYPE strings are compared by FNV hash
  ("0" 350ca8af snipe … "6" 330ca589 land). Property formats are `%d` and
  `%f`; the CommandPost sentinel is the empty string. Radius copies sit at
  (sin a, 0, cos a) × radius for a = 0, 90, 180, 270 degrees (0 degrees along
  +z), facing the center.
- **GroundGotoHelper.** State 8 (straight) does not set the destination, so the
  jet attempt there measures against the previous one. Goto stops the unit
  until the initial local path arrives.
- **State-4 corner detour can stall.** For non-infantry direct moves the
  detour keeps one side for the whole move and takes, on that side, the
  corner at the widest angle from the heading. Standing on a corner, that is
  the corner underfoot, so the detour point stops changing, no new path is
  issued, and with no executor there is no stuck status either. Equal corner
  distances at the initial obstruction pick the second side, which can be the
  one away from the goal. This is the original game's behavior and is kept
  in Classic; tests cover a geometry where the detour succeeds.
- **Planner details.** The fast straight-segment answer needs the owner's
  outstanding-segment byte at 10 or less; queued priority is base + that byte
  and only 0..10 are ever served (consuming segments lowers a queued
  request's priority). Request selection unlinks dead owners' requests at
  each visited priority. The jet's deadline is a float32 (`now + 5 + 10U`,
  then `+5` per expiry).
- **Boundary orientation.** With the half-plane rule "inside when every
  dot(p − start, (−dz, dx)) <= 0", every command post of nab2, bes2, kam1,
  geo1 and hot1 is inside its map's boundary.

## Battlefront II (2005) navigation

Support for 2005 maps lives in `rust/sim/src/ai/nav/swbf2/` (`mod.rs`,
`graph.rs`, `cursor.rs`, `goto.rs`, `load.rs`), kept separate from the released
2004 navigation modules so 2004 behavior and digests stay unchanged.

### Admission payload (schema 2)

Battle admission selects the navigation reader from the battle's game family.
The 2004 reader receives its original bytes. The 2005 reader receives a
schema 2 row with the map's planning graph, blocking groups, barriers, hints,
boundary names, paths and regions, even on maps without a graph. The optional
planning binary supplies the blocking groups' routing changes.

### Floors, corridors and orders

A hub's XZ position locates its disc on the planning plane. At map admission,
downward collision probes find supported levels in that disc. An isolated
soldier mover tests each directed corridor at 60 Hz, including launch corridors
and corner detours. Its final position must be grounded on the destination
level. A hub can therefore serve two floors at the same horizontal position.

Straight recovery moves and direct approaches clip a local segment against
barriers and collision, backing off 0.25 m from an obstructed barrier face and
failing when a request over 0.5 m leaves less than 0.2 m of walkable progress.
They follow the floor in the requested horizontal
direction. Route orders use the planning graph. Combat approaches can end on
a supported position within sight of their target. Capture approaches can end
within the capture area's acceptance distance.

The point query and the routed ground order share the same attachments, corridor
directions, class bits and enabled barriers. Both endpoints require support and
headroom. A clear segment can shorten an approach within a supported floor or a
straight ordinary corridor in its permitted direction.
A nearby alternative disc must also be reachable in the appropriate direction.
Outside either endpoint's disc, the search compares reachable entrances by
total walking length, including the approach and the exit.
The helper submits one waypoint at a time. A soldier already standing on the
entrance can enter the corridor immediately. Inside the one-metre infantry
arrival circle, the helper can hand off to the next ordinary segment after
checking it from the actual feet. An obstructed turn keeps its current
waypoint, and an airborne segment retains its certified launch position.
In a 2005 battle a soldier off the ground keeps the leg it left the ground on:
the leg does not finish in the air, and an order given in the air is planned
where the soldier lands. In the air the 2005 stick accelerates the body with no
speed limit, so the bot steers for the velocity that brings it down on its
waypoint (the time to the waypoint's height under gravity 18) instead of
straight at it, at full stick while a metre a second or more is missing.
On a corner the soldier can steer
to a validated bend. With no viable leg, or after sustained lack of progress, it
clears the movement request and reports failure. Final arrival checks horizontal
range and height on the supported destination floor. For nonzero ground segments the
native clearance query also drives a separate, preallocated soldier mover.
Ordinary descents require floor support through the descent, including when
the capsule briefly leaves the ground on a slope. If a walking segment has
no support under the centre of the starting footprint, the floor check
samples 0.25 m away in the order +X, -X, +Z, -Z. The mover still starts
at the soldier's feet. A separate destination needs support at its centre.
A certified climb on a
two-way connection may also have a descending leg with up to six metres of fall.
A straight walk in a 2005 battle (a recovery move) may step off a ledge deeper
than that, down to the eight metres the floor search looks below; the same
physical drop check passes it with that limit. A recovery that has stood three
seconds in one spot (the game's stall time) with no checked step left walks on
as the game's straight walk does: two metres toward its point, its own
collision deciding, over a floor that lies below the end within those eight
metres and through no solid barrier. A soldier that can jet out under open sky
does that instead.
An authored jump must reach its landing floor. Under an overhang the survey
can place an exposed touchdown beyond the landing hub and certify a walk back
to it. Every distinct admitted soldier jump profile must complete the airborne
segment. A changed collision body on the jump requires fresh certification.
Above 32 distinct profiles the survey declines ordinary jump links. In interiors, room-sector polygons
bound the floor search. Long multi-level arcs also sample near-endpoint bends
to locate ramps situated close to either hub. Door passages are surveyed with
the door open. Barriers and planning groups gate locked doors, while surveyed
proximity-door passages remain plannable so a soldier can approach and wait
briefly for the shutter to open before crossing (failing cleanly if it stays
shut). Current colliders still govern every live segment and every non-door
corridor.
The junction uses a supported point within its disc that clears the enabled
barriers. Jump corridors use the storeys
connected to the approach and exit. An ambiguous launch or landing is omitted.

Blocking a planning group or changing an authored barrier invalidates the
destination rows. A ground leg crossing a restored barrier or a newly blocked
arc stops immediately. A body flag change rechecks ground legs near that body.
When only moving bodies join, leave, swap, or change enabled flags in collision
world order while the static bodies keep their positions and order, navigation
clears its directed segment cache and flags overlapping active legs for recheck
without discarding surveyed static corridors, door passages, or warmed
destination rows. A static body flag change revokes only the surveyed door
passages and floor corridors whose segments meet that body, keeping unaffected
proximity doors and warmed destination rows intact when no corridor changes.
Other controls keep their movement requests. Native approach queries also check
the collision world order and enabled-body state before reusing previously
tested segments.

The five low barrier bits apply to the five traversal classes. The enabled
state is independent. Map admission composes base-world barriers and hints
followed by records in the mission's active layers. Hints retain their ordered
properties and the additional types 7 and 8 participate in capacity assignment.
Each native hint's source selects its original record in the composed array,
including after radius expansion. That record retains its package-local
`sourceIndex` and, for a layer, the enclosing package key in `sourceLayer`.
Barrier records use the same package-local convention.

### Route queries and storage bounds

During a bot decision, `ai::control::world::World::nav_route_distance(from, to,
radius, mask)` returns a route status, distance and hop count from the navigation
and collision owners already borrowed for that phase. Distance is in metres.
The 2004 reader and helper use their released storage. Its optional distance
query explores positive baked choices for the destination without consuming a
random draw. With a 2005 graph, a point query searches the supported corridors
using deterministic shortest distances and stable indices. Destination rows
and working arrays are allocated on admission. Small graphs precompute soldier
rows for radii up to 0.30, 0.40, 0.45, 0.48 and 0.50 m. Each row certifies the
upper bound of its band. Larger radii retain their exact footprint. Endpoints
and local approaches always use the asking soldier's radius.

Navigation admits four soldier route questions across all callers per simulation
tick, including four before the first tick. Further calls return `Unknown`
immediately. Each admitted query and each movement order may perform 16 uncached
mover checks and examine 400,000 row entries. An order shares those credits
across its exact destination, objective or combat approaches, and replans.
Across all route questions and order searches on a tick, navigation also caps
the examined row entries at 400,000. Exhaustion of an order's own credits ends
its search. A query can return a route already proved. When a replacement Route
order exhausts its credits, it reports failure and lets the soldier finish the
current certified leg. A soldier starting from rest reserves one of the same 16
checks for a short local approach.
Both paths signal the controller when the order is refused. An active executor
keeps steering and reports its own status. Once it finishes, the controller
counts time without an executor and can request recovery after five seconds
for a soldier or six seconds for a droideka. Installing a new leg clears the signal.
On a doorway approach with multiple surveyed bends, a moving soldier finishes
the turn if its velocity still carries it away from the outgoing leg.
Checking the execution of live legs is limited to 512 physical checks per tick.
A changed leg waits for clearance before the executor can steer along it again.
Direct walking checks made by combat before it orders a pursuit are limited to
16 per tick across all soldiers. Exhaustion declines the direct walk and combat
can request a Route order. Native combat callers use
`owned::live::direct_walk_clear`.

External callers outside the bot controller (such as space hangar pilot routing)
can walk a soldier to a world point on a 2005 map through
`World::nav_walk_begin`, `World::nav_walk_step`, and `World::nav_walk_stop`,
returning `NavWalkStatus::{Waiting, Moving, Arrived, Failed}`. Each unit slot
receives a pre-reserved external control and `Swbf2GroundGoto` at 2005
admission (`ai_world_begin_swbf2`), isolated from the controller's own helper
so `ai_world_phase_in` and post-AI callers do not overwrite each other's routes.
External walks share `GroundProbe`'s 10,000-unit per-tick collision allowance,
participate in `Navigation::resume_walks`, and overwrite movement, posture,
jump, and jet intent on `Moving`, `Waiting`, and `Arrived` ticks. Navigation
admits at most 2 new external walk starts per tick across all callers; targets
on disconnected floor groups are rejected by `Floors::may_connect` before any
floor raycast, mover check, or start credit is spent. In a 2004 battle no
external walk slots are reserved and `nav_walk_*` immediately returns `Failed`
(or no-ops on stop).

A search first compares the levels near both ends. Levels joined by corridors
form a group, whatever the direction or present state of those corridors. When
no entrance in reach of the start shares a group with an exit in reach of the
goal, no search can connect them and no physical check is made. This answers
questions between two ships, where every deck is its own group.

The admission survey and the live battle run the same ground check: the floor
under the line, then a private mover driven along it. In the battle all checks
of a tick draw on one allowance of 10,000 units of collision work, about two
milliseconds on the reference x86-64 machine. A check costs four units to begin.
A ray costs three and one more for every 40 enabled bodies. A mover step costs
three, one more for every three moving bodies in the world, two for each
triangle around the part of the body that met the world last, and 200 more when
it tries the space over a step ahead. The charge depends only on the collision
world and is the same on every machine. Route plans stop at half of the
allowance and straight or direct steps at two thirds. Legs under way, route
questions and direct walks can use all of it.

A tick also runs at most six searches from a point to a point (each finds the
floors' attachments at both ends), shared by every order. When an order's own
point has no route, its plan tries the objective's floor and then sixteen
approaches around the point; an order that finds no search left waits, and its
plan goes on in a later tick with the places it has not tried yet.

A plan, a step or a leg that runs out of work waits for the next tick. Its
check stops between two steps, keeps its own private mover, and goes on before
any other check of its kind, whoever asks first. The answer is kept for the
order that asked. A check without an answer is neither clear nor blocked: it is
not cached and costs the order no credit. A change of the collision bodies or
of their enabled flags drops a stopped check and a kept answer, and so do eight
ticks in which nothing asks for that kind. Route questions and direct walks do
not wait. Without work left a question is `Unknown` and a direct walk declined.

At the start of every tick navigation goes on with what is waiting: first with
soldiers that stand on a leg, then with orders from the longest wait to the
shortest. New orders of the tick come after them. A waiting plan is searched
from where it began while the soldier stays within two metres, so that each
tick goes on with the checks of the last one. Its first leg is still checked
from the soldier's feet. Meanwhile the soldier walks its present leg to its
end. A newer order for the same control, the soldier's death or a seat ends the
wait. After one second of waiting the order is refused as above.

A soldier can attach at a checked projection along a directed straight or single-bend corridor (including ramp bends), and destination attachment filters candidates to the connected floor groups reached by the start attachments so isolated floor nodes never shadow reachable corridors. Without
a graph, a successful direct walk also returns `Reachable`. A goal whose centre
is inside an enabled barrier, or two disconnected discs whose plausible local
attachments preserve the same disconnection, can establish `Unreachable`.
A start overlapping a barrier, ambiguous junction, missing attachment, or
unsupported traversal mask can yield `Unknown`. Goal selection can try an
unknown destination and rank a proven unreachable destination last.

`Swbf2Graph::hub_route_distance` describes authored topology. Ground planning
uses the physical corridors above. The routing choice made by `next_arc` from
the packed slot and weight words is **provisional**. An indexed weight-group change applies its checked XOR delta. A named
connection in a multi-connection group switches independently. Both operations
update the authored distances and invalidate the affected route choices.

Admission accepts at most 256 hubs and 65,535 authored arcs, eight supported
levels per hub and 131,072 directed floor corridors. A shared 262,144-unit budget covers floor pairs, bend candidates, doorway
samples and tested indoor links. An indoor opening first uses 56 candidate sites. A missing approach can
search two deck patches of at most 65 by 65 sites each, meeting at the typed
door. Every admitted segment passes the same barrier and mover checks. An
ordinary candidate needs at most two movement drives, each bounded by segment
length. Up to 4,096 indoor passages store six intermediate points apiece. A
corridor record contains
two level indices, an arc index, an optional bend and a length. Each corridor
has one reverse index for the destination search. Each destination row uses one
index, one length and one 16-bit hop count per level. The rows share an 8 MiB limit on those arrays. A small
graph can reserve one row per floor and soldier band, plus a scratch row.
Larger graphs retain at most 256 cached rows. The native
clearance cache has 4,096 directed segment entries and room for the collision
world's enabled-body flags. Five packed words per ordered hub pair occupy at
most 1.25 MiB. Up to ten authored distance/hop tables occupy at most 3.75 MiB.
The reusable authored search heap holds at most twice the arc count plus one.
Up to sixteen group bits select admitted deltas with at most 32 MiB of decoded
bytes in total. The authored barrier grid admits at most 1,048,576 cell entries.
The active packages together admit up to 65,535 source records of each kind
and 262,144 property rows. Records share a 16 MiB budget charging 16 bytes per
JSON value plus the UTF-8 lengths of keys and strings, including layer keys.
Admission checks block boundaries, row widths and decoded
lengths. Toggles and searches reuse their existing buffers.
