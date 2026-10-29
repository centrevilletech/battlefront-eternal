# Classic AI: navigation

Navigation for the Classic AI (`docs/ai.md`): the extracted planning data
(`tools/zero_ai.py` → `public/maps/<map>/ai-nav.json` + `ai-nav.bin`) and the
runtime under `src/ai/classic/nav/`. Behavior follows the 2004 game's route
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

```js
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

```js
graph: {
  info: [hubCount, arcCount, third],     // INFO: i16, i16, u16 as stored (third is 5 in stock data)
  hubs: [{
    name: "Hub0",                         // char[16], up to the first NUL (latin-1)
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

```js
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

```js
barriers: [{
  name: "barrier12",                      // INFO/NAME
  xfrm: [12 floats],                      // INFO/XFRM, raw (translation z NOT negated)
  size: [sx, sy, sz],                     // INFO/SIZE; x/z are footprint half extents
  flags: 0x3f,                            // INFO/FLAG u32 traversal mask
  properties: [[hash, name, value], …],   // BARR-level PROP children (the loader reads and ignores them)
}]
```

The original game's barrier loader negates the translation's z and keeps the
basis rows as stored. The runtime applies that adapter (`barrierFromRecord`);
the extractor writes the file's values.

### `boundary`

```js
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

```js
paths: [{
  name: "CP1Spawn",
  info: [count, word2, word3],            // i16, i16, u16 as stored; count must equal the points
  points: [[x, y, z, qw, qx, qy, qz], …], // PNTS: position then quaternion w,x,y,z
  prop: "6e756d…" | null,                 // raw PROP bytes, hex (not hash/string pairs; unused at runtime)
}]
```

### `regions`

```js
regions: [{
  type: "box", name: "cp1_capture",
  xfrm: [12 floats], size: [sx, sy, sz],
  properties: [[hash, name, value], …],   // optional hashed PROP children
}]
```

Duplicate names stay separate entries.

### `counts`

```js
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
| `hubSlotGap` | A hub lists an arc after an unused (255) slot (`hub, arcs`); the alternate-arc fallback stops counting at the first unused slot. |
| `hubMaskDiffers` | The OR over the hub's slots differs from the OR over every arc touching the hub (`hub, slotMask, touchingMask`). |
| `zeroLengthArc` | Coincident arc endpoints (`arc, hubs`); the corridor test would divide by zero, so the runtime skips these arcs. |
| `hintType` | Unsupported hint TYPE (`hint, name, type`), dropped at load. |
| `jetForward` | A jet hint forward vector that is not unit length (`hint, name, length`), kept as stored. |
| `barrierBasis` | A barrier basis that is not orthonormal (`barrier, name, xLength, zLength, dot`), kept as stored. |
| `boundaryPath` | A boundary Path reference with no matching PATH (`config, hash`). |
| `duplicatePath` | Two PATH entries hash to the same name (`hash, paths: [first, later]`); lookups take the first. |

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

## Runtime modules (`src/ai/classic/nav/`)

Everything is re-exported from `index.js`. Modules import only `three` and
each other.

| File | What |
| --- | --- |
| `index.js` | `ClassicNav` (world.nav) and the re-exports |
| `data.js` | `loadNav(json, bin, options)`, `routeWords(bin, routes)` |
| `graph.js` | `NavGraph`: attachment, weighted next-arc selection, alternate arc; `containsHub`, `containsArc`, `corridorGap`, `otherEnd` |
| `cursor.js` | `GraphCursor`: one graph route (initialization branch tree, advance, shell waypoints, blocked-waypoint retry) |
| `barriers.js` | `barrierFromRecord`, `BarrierSet` (grid broadphase and the planner queries), `inside`, `corner`, `crossing`, `segment`, `ejection`, `detour` |
| `search.js` | `BarrierSearch` (incremental best-first corner search), `OpenHeap` |
| `planner.js` | `PathPlanner` (request queue, shortcut, service), `AIPath`, `Pool` |
| `executor.js` | `PathExecutor`, `lookahead`, `nearRadius`, `passRadius`, `flyerCircle`, `EXEC` thresholds |
| `control.js` | `NavControl` (a unit's path and request state), `MovementRequest` |
| `goto.js` | `GroundGotoHelper`, `FlyGotoHelper` |
| `analogs.js` | `analogs`, `alongPath`, `coefficients`, `navigationInput` |
| `hints.js` | `buildHints`, `HintSet`, `pickStance`, `pickSecondary`, `HINT_BITS`, `STANCE` |
| `boundary.js` | `Boundary` |
| `units.js` | `KIND`, `SIZE_BITS`, `traversalMask`, `traversalRadius`, `nominalSpeed`, `DEFAULT_RADIUS` |
| `debug.js` | line-segment builders for an overlay: `graphSegments`, `barrierSegments`, `boundarySegments`, `hintSegments`, `pathSegments`, `lines` |

### `ClassicNav` (world.nav)

```js
const nav = new ClassicNav(json, bin, { posts, hintCapacity, pools });
// ClassicNav.empty(map) for a map without ai-nav.json: world.nav is required.
// posts: { byName(name) -> post, list: [{ position: Vector3, ... }] } for hint
//   CommandPost names and the Land-hint association; optional.
// pools: { paths: 900, pathNodes: 7200, requests: 75, searchNodes: 1024, heap: 512 }
nav.graph          // NavGraph or null
nav.barriers       // BarrierSet
nav.hints          // HintSet (the loaded list: radius rings expanded, pool rules applied)
nav.boundary       // Boundary
nav.paths          // [{ index, name, info, points: [[x,y,z,qw,qx,qy,qz], ...] }]
nav.regions        // the ai-nav.json records, untouched
nav.planner        // PathPlanner
nav.issues         // runtime reports (zeroLengthArc, routeHang, routeTable)
nav.service(budget = 1)        // one planner service
nav.idle                       // no search running and nothing queued
nav.control(controller)        // controller.navControl, created on first use
nav.helper(controller, world)  // GroundGotoHelper, or FlyGotoHelper for Flyer bodies
nav.outside(point)             // the boundary leaves the point outside
nav.path(name)
nav.queryHints({ mask, center, halfExtent, cap, requester, now })   // HintSet.query in object form
nav.hintAvailable(hint, now) / nav.markHint(hint, owner, now) / nav.plannerIdle()
```

`classicBinding(ai)` (exported) returns the services `src/ai/classic/index.js`
binds against `ai.world.nav`: plannerIdle, service, pointClear, clipSegment
(barrier obstruction, shortening the end), traversalRadius, traversalMask,
queryHints, hintAvailable, markHint, analogs, navigationInput. It tolerates a
partial stand-in nav.

Scheduler (Controller): per AI cycle, `if (!nav.idle) nav.service(1); else
losBudget *= 2;` then, if the deferred-LOS service returned false,
`nav.service(1)` again. The budget is planner barrier tests (10 per 1.0), not
paths or units.

### Movement helper (docs/ai.md interface)

```js
const helper = nav.helper(controller, world);   // controller.unit is the unit it drives
helper.goto(point)                     // "route": graph route over 50 m, else one local path
helper.goto(point, { mode: "direct" }) // state 3 for Soldier/Droid (cut 1 m before a barrier,
                                       //   five retries), the state-4 corner detour otherwise
helper.goto(point, { mode: "straight" })  // state 8: a raw two-point path
helper.follow(ref, distance, remembered)  // state 5 (Soldier/Droid): ref = { unit, generation }
helper.stop()
helper.tick(dt)            // the helper FSM; writes controller.request
helper.arrived             // per state (see goto.js)
helper.status              // executor status: 0, 1 no improvement for 5 s, 2 stationary 3 s (Droideka 6/6);
                           //   also 1 for a soldier or droideka stranded 5 s (6 s) without a path (below)
helper.failed              // the planner answered "no path" for the current goto
helper.stopped             // state 7
helper.state               // 1..8
helper.destination         // Vector3, after correction out of barriers
helper.destinationPoint(out, current)
helper.stance(0 | 1 | 2)   // stand / crouch / prone, applied by navigationInput
helper.jet(vector)         // jet request; zero = the soldier's own launch direction
helper.aimAt(point) / aimAtTarget(ref) / aimAlongPath()   // controller.aimState 1 / 0 / 2
                           //   (controller.lookAt / aimAt / lookAlongPath when present)
```

`goto` replans and stops the unit until the first local path arrives, as the
original game's does: call it when the destination changes, not every tick.

`world` needs `nav`, `random` (ClassicRandom, the AI generator), `time`,
`units.resolve` (for follow), and optionally `gameRandom` (the second
generator, see Findings), `los(from, to, a, b)` or `collision.cast` (jet
clearance). `options.remembered(controller, unit)` supplies the remembered
point for `follow(..., remembered = true)`.

### AI input step

```js
// Tick order step 2, every AI unit, every simulation step h:
navigationInput(controller.navControl, unit, unit.intent, h, aimPoint, { facing: controller.facing });
```

It runs the unit's path executor, writes `intent.forward/right/turn/pitch`
from `controller.request` and the aim point, then applies a pending jet request
(jump, then jump again in the jump state, which lights the jetpack;
`intent.jetDirection` for a nonzero request vector) or the requested posture as
`intent.crouch/prone` toggles. `aimPoint`: Combat's target-plus-lead point for
aimState 0, `controller.aimPoint` for aimState 1, `null` for aimState 2 (30 m
along the path, or straight ahead near the end). `analogs(unit, request,
aimPoint, dt, out, { executor, facing })` alone does just the conversion; pass
the unit's executor (or null) so the end-of-path slowdown is right.

Coefficients: forward MaxSpeed, strafe MaxStrafeSpeed, yaw MaxTurnSpeed and
pitch MaxPitchSpeed, the port's `classDef` fields of the same names (falling
back to the Soldier's `maxSpeed`, `maxStrafe`, `maxTurn`, `maxPitch`). Signs:
the original game's strafe and yaw analogs map straight onto the port's
`right` (moves along (−cos yaw, 0, sin yaw)) and `turn` (positive decreases
aimYaw), and its pitch analog onto `pitch` (positive raises the aim).
Derivation: the transform's first row is up × forward = (cos yaw, 0, −sin yaw),
and the conversion's b = −(F × d).y must move the unit toward its target; tests
drive a kinematic soldier with the port's axes to its goal.

### Other helpers

- `NavControl`: `attach(unit)`, `request`, `executor`, `status`, `segments`,
  `publish(path, ready)`, `consume(mode)`, `install(path)`, `append(path)`,
  `stop()`, `update(dt)`, `jet { pending, vector }`, `jetting`, `stance`,
  `avoidSide`. One per controller, kept across lives; `attach` resets it for a
  new unit or generation.
- `HintSet.query(mask, center, half, cap, out, owner, now)`: the generic box
  query (mask of hint bits; with an owner, hints reserved under 4 s ago are
  skipped; load order). `available(hint, now)`, `reserve(hint, owner, now)`
  (a hint and its ring). A HintSet is iterable and has `list` and `length`.
  Hint fields: `kind, bit, name, position, right, up, forward` (as authored),
  `primaryStance` (stance bit mask: 1 stand, 2 crouch, 4 prone),
  `secondaryStance`, `secondaryChoices` (two-bit side mask), `mode`,
  `radius`, `radiusEnabled`, `registerRadius`, `commandPost`,
  `commandPostName`, `reservedAt`, `reservedBy`, `copy` (0..3 for ring
  copies), `source`, `index`; ring links `link` = `next` and `siblings`
  (the ring); aliases `primaryMask`, `secondaryMask`, `sideMask`.
  `pickStance(mask, random)` redraws (0 to 2) until the drawn stance is in the
  mask (stand for an empty one); `pickSecondary(choices, random)` does the same
  over the two-bit choice mask (-1 for an empty one).
- `traversalMask(unit)`: `unit.navMask`, else `classDef.AISizeType` (string,
  case-insensitive; unknown → Soldier), else 1. `traversalRadius(unit)`:
  `unit.navRadius`, else a quarter of the horizontal diagonal of
  `classDef.bounds` ([[min], [max]], the model's INFO box; half for Hover),
  else 0.476.

## Integration (changes outside `src/ai/classic/nav/`)

1. `tools/extract.py`, end of `extract_map` (after `map.json`):
   `from zero_ai import export_map` and `export_map(name)`. A `ValueError`
   means malformed planning data; report it rather than abort the map.
2. Soldier contact avoidance (`src/soldier.js`, and the same in
   `src/droideka.js`): add `this.avoidance = new THREE.Vector3()` and, where a
   polygon contact is resolved (`touchPolygons`, before `contact()`), when
   `Math.abs(this.contactNormal.y) < 0.3` add the contact normal (pointing
   from the geometry toward the soldier, unweighted) to `this.avoidance`.
   Terrain contacts (`touchTerrain`, category 64) do not count; soldiers are
   category 2 and do not count either. Static buildings and props (category
   8) and bodies (vehicles 4, live props 8) do. Do not clear it per tick: the
   path executor consumes and zeroes it.
3. Jet direction (`src/soldier.js startJet`, `src/units.js blankIntent`):
   `intent.jetDirection` (Vector3 or null). For a non-player soldier a
   nonzero `jetDirection` replaces the launch direction literally (not
   normalized); null keeps the ordinary stick direction.
4. Class data for units: `bounds` (model INFO box, `zero_model.BOUNDS[GeometryName].box`)
   and `AISizeType` when authored (Combat's class extraction has it) on
   `classDef`, or set `unit.navRadius` / `unit.navMask` directly.
5. `world.gameRandom`: a second ClassicRandom for the game generator (see
   Findings). Without it the jet attempt uses a private stream.

## Tests

- `node --test tests/ai-nav.test.js`: synthetic graphs, barriers, search,
  planner, executor, steering, analogs, hints, boundary and the ground helper
  driving a kinematic soldier; and, when `public/maps/nab2` is extracted,
  graph routes between the nearest hubs of every command-post pair and a
  soldier walking cp2 → cp3.
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
| `barriers.js` | Broadphase candidates in source order, each once, by bounding circle (radius: the length of SIZE) against the query box | The original game's grid visiting order and duplicate visits are not reproduced; only nearest-hit ties and work counts can differ |
| `analogs.js` | The AI input's posture/jet timer (a Controllable field in the original game) is read as `soldier.lock > 0` | The field's port equivalent is not confirmed |
| `hints.js` | Box query returns hints in load order | The original game's spatial order is unknown; only matters when `cap` truncates |
| `cursor.js` | No compatible arc anywhere: the cursor goes straight to the exact destination | The original game has no defined behavior there; not needed for stock data |
| `goto.js` | `follow(..., remembered)` needs `options.remembered(controller, unit)` from Combat's memory | The remembered point lives in perception, which has to expose it |
| `zero_ai.py` | Duplicate PATH names resolve to the first in source order | The original game's path list order is unknown; only yav2 has duplicates |

Safe-port divergences (unsupported input only; stock data never hits them):
a routing word the selector could never finish returns no arc and reports
`routeHang`; a mask with no routing table (bit 32 and up) is refused
(`routeTable`); a full open heap refuses the insert and counts an overflow
(the original game writes past its 512 entries); a detour side with no corner
is skipped (the original game recurses on an uninitialized point); FSM
transition drains stop after 16 passes; empty paths complete at once.

Port changes against bots that stop moving (found with
`tools/ai-stuck-probe.mjs`, see docs/ai.md "Stuck bots"; tests in
`tests/ai-stuck.test.js`). Each departs from the original game where its
behavior, or the port's arithmetic, left a bot standing for good:

- **Stranded routes** (`control.js`, `goto.js`). When the helper gives up
  short of its destination (the planner answered no path, the graph route
  ran out of arcs or lost its waypoint, or a follow's direct path ran out
  more than 2 m from a target that keeps still), `NavControl.stranded` is
  set; with no executor the control then reports status 1 once the
  executor's own no-progress time has passed (5 s, 6 s for a Droideka), for
  soldiers and droidekas only. Stagnation recovery (priority 8) takes it from
  there. The original game reports 0 without an executor, so a bot whose route
  failed stood where it was. New orders (`goto`, `stop`, a new `follow`,
  `installPath`) and any installed path clear it; stranded again within 2 m
  of where it was, the time already spent there counts on (a fighter's look
  round between two failed routes from one barrier pocket restarted the
  count each time, and it never reached 5 s). The count starts over once
  the unit is more than 2 m away, and with each life.
- **A full request queue** answers the request as a completed failure
  (`goto.js`); the original game's refusal left the helper waiting in state 2.
- **Starved requests** (`planner.js`): a queued request passed over while
  `STARVED` (20) later searches finished is taken next, oldest first, if its
  priority is one the queue serves (10 or less). Strict priority let a
  stream of priority-0 asks (fighters repositioning) hold a route's first
  leg (priority 1) back for as long as a fight lasted.
- **Droideka start** (`analogs.js`): the 0.2 m stop applies only when the
  point it steers for is the waypoint itself. At rest its lookahead lies
  exactly 0.2 m on, where float rounding alone decided between moving and
  never moving.
- A dead or unbound bot's path work leaves the planner with it
  (`UnitController.unbind` resets the control); before, its search and
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
  separate generator state (`world.gameRandom`, Integration 5).
- **Shell direction draw order.** A hub-shell direction draws three uniforms;
  the first becomes z, the second y, the third x, each `2 × (U − 0.5)`.
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
  (sin a, 0, cos a) × radius for a = 0, 90, 180, 270 degrees (the first along
  +z), facing the center.
- **GroundGotoHelper.** State 8 (straight) does not set the destination, so the
  jet attempt there measures against the previous one. Goto stops the unit
  until the first local path arrives.
- **State-4 corner detour can stall.** For non-infantry direct moves the
  detour keeps one side for the whole move and takes, on that side, the
  corner at the widest angle from the heading. Standing on a corner, that is
  the corner underfoot, so the detour point stops changing, no new path is
  issued, and with no executor there is no stuck status either. Equal corner
  distances at the first obstruction pick the second side, which can be the
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
