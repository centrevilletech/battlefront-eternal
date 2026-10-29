# World CPU performance

The engine pass profiles actual Theed and Bespin scenes before choosing changes.
The initial 1,080-frame walking runs, with drawing disabled, put Theed foliage
at 1,844 ms and collision raycasts at 564 ms. Bespin spent 407 ms in raycasts.
These timings include diagnostic wrappers, exclude GPU rendering, and describe
these routes rather than every gameplay workload.

The changes preserve the collision geometry and foliage artwork:

- Live collision bodies keep their world-space bounds. Sphere and ray queries
  reject distant bodies before entering their BVHs. Ray traversal also stops at
  the nearest already-known hit. Enabled/disabled state and ignored firing
  turret bodies still participate in selection.
- Polygon contact records, triangle bounds, and shapecast callbacks are reused.
  A new gather resets all consumed-contact flags and triangle lists.
- Each foliage class computes a card's camera-facing corners, width, and sway
  once per frame. Repeated placements translate those corners. Grass keeps its
  per-placement sinking fade; leaves keep per-leaf distance fades, tint, atlas
  coordinates, and mirroring. No leaves or instances were removed.
- Bolt textures, tint and batch handles are cached per ordnance definition.
  The regression workload needs two texture lookups across 1,200 bolt updates,
  rather than two on every update. Empty particle batches skip upload bookkeeping.
- Dynamic light requests reuse records and copy each emitter position. This
  removes repeated request allocation and fixes lights following the last
  shared scratch position. Enhanced effects use fixed particle pools.

## Measurements

Interleaved A/B runs use the same extracted maps, enabled live bodies, soldier
controller, seeded foliage layout, input, and camera path. Reference modules
were copies taken immediately before this optimization. Times are milliseconds.

| CPU workload | Before median | After median |
| --- | ---: | ---: |
| Theed: 1,800 uphill ticks and 1,800 aiming rays, 23 enabled live bodies | 283.50 | 135.22 |
| Bespin: 1,800 uphill ticks and 1,800 aiming rays, 14 enabled live bodies | 513.48 | 222.90 |
| Theed foliage: one update, 3,600 camera-sweep samples | 1.935 | 1.373 |

Both movement runs produced identical final positions. The foliage sweep drew
1,915 to 3,950 cards per frame; every compared position, color, and UV matched
exactly. Foliage p95 fell from 5.721 to 4.371 ms. Concurrent work on the machine
causes timing variance; the benchmark intentionally has no timing pass/fail
threshold.

In the complete browser update loop, the same 1,080-frame Theed route changed
from 3,411 to 2,769 ms and Bespin from 1,284 to 995 ms. Theed polygon gathering
fell from 55,152 BVH traversals to 2,298; Bespin from 32,400 to 2,160. Raycast
call counts were unchanged. These browser samples include other engine systems
and concurrent changes, so use the isolated A/B workloads to attribute gains.
None of these measurements establishes a rendered FPS increase: GPU cost,
resolution, effects load, and frame pacing still matter.

## Reproduce and check

With the local extracted `public/maps/nab2` and `bes2` assets:

```sh
node tools/benchmark-world.mjs
```

The script loads the real collision geometry and live-prop states, performs
warmups, and reports medians and p95. `PERF_ROUNDS`, `PERF_CYCLES`, and
`PERF_FRAMES` control sample counts. Optional `PERF_REFERENCE_COLLISION` and
`PERF_REFERENCE_FOLIAGE` paths select reference ES modules exporting
`CollisionWorld` and `Foliage`; their normal imports must resolve from their
location. Providing a reference adds interleaved A/B timing and output checks.

Behavior regressions do not need reference files:

```sh
node --test tests/collision-queries.test.js tests/foliage-equivalence.test.js tests/foliage-buffers.test.js tests/movement-continuity.test.js
node tests/collision.mjs
node tests/foliage.mjs
```

The query test compares 700 randomized gathers, clearance probes, and rays
against brute-force geometry, including toggled and ignored live bodies and
terrain. Foliage tests retain the prior card equations as an output oracle and
compare 49,195 visible cards across camera pitch/yaw, distance settings, grass,
vines, trees, and zero/30/60/144-Hz updates. Existing traversal tests preserve
arch clearance, stairs, low ceilings, tall walls, and thin-wall collision.
