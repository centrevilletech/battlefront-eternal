# Renderer and client-controls measurements

This document records earlier rendering and input measurements. Its before/after
figures belong to the stated workloads and builds, not to the complete current
release. Battle drawing, resource lifetime, camera/light math and frame
scheduling belong to `rust/bfe-render` and `rust/bfe-presentation`; see
[rendering.md](rendering.md).

## Optimizations measured

- Canopy camera selection keeps its candidate storage and cached distances.
  Leaf instance attributes upload only when the group occupying a slot changes,
  and uploads cover that slot rather than the entire capacity. The original-card
  mask updates only when the selected groups change. Near, far, and reflection
  cameras still select independently with the original frustum/range equations.
- Opaque static color passes reuse the existing spatial shadow cells to omit
  off-screen triangles. Two cached camera ranges cover the main and reflection
  views. Original triangle order, vertex attributes, source indices, and
  collision data are retained. This costs two private color index ranges per
  eligible batch; it does not add draws or reduce visible detail.
- Shadow culling replaces temporary boolean arrays and string signatures with
  four persistent visibility arrays, one per cached shadow camera. Empty results
  update the draw range without uploading an unused index buffer.
- Lamp selection keeps the same stable, nearest-four normalized-distance ordering
  and the same occlusion decisions, using reusable bounded insertion storage
  instead of allocating and sorting every source. Settled lamp discovery skips
  its empty texture-readback work.
- Particle and water captures read retained target handles instead of querying
  `FRAMEBUFFER_BINDING` inside a draw. The Rust resource owner supplies target
  storage, binding, copies and resolves.
- GPU timer queries remain asynchronous, with status polling reduced to the
  existing eight-frame sampling cadence. Disjoint samples are discarded.
- Full-screen post passes omit redundant clears; their triangles already
  overwrite every pixel with blending disabled. World, shadow, and HUD clear
  behavior is retained. Stable camera projections and first-person light-source
  storage are reused.
- Input resolves key/button bindings once and reads axes without temporary
  closures. Focus loss clears queued presses as well as held keys, preventing
  an old action from firing when the player returns.

There is no `glFinish`, pixel readback, or extra geometry pass in production
frame rendering. Synchronization and image capture below belong only to
benchmarks and tests.

## Isolated CPU measurements

Seven measured rounds after one warmup, alternating version order, on the
shared development host. Values are medians in milliseconds for each whole
workload, **not frame times**. Both implementations use identical fixtures.

| Workload | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| 100,000 input press/take/axis/hold cycles | 365.91 | 61.66 | 83.1% |
| 5,000 lamp selections among 250 sources | 149.57 | 30.76 | 79.4% |
| 5,000 shadow-camera culls over 400 boxes | 64.10 | 24.36 | 62.0% |
| 2,000 canopy camera updates over 20 groups | 564.10 | 22.01 | 96.1% |

The canopy workload includes buffer preparation and selection but no GPU draw.
The benefit depends on how often the selected groups change. These reductions
do not translate directly into the same percentage improvement in total FPS.

## Complete render measurements

Measured on a software renderer at 320x200, CP3 spawn, 4x MSAA plus FXAA, low
shadows, bloom/soft focus/enhanced water and foliage enabled, including HUD
rendering. Each map/version measures three alternating 16-frame camera sweeps
after warmup. Times below are milliseconds, including diagnostic wrappers. They
are **not hardware GPU timings or display-paced FPS**.

| Map / metric | Before | After |
| --- | ---: | ---: |
| Theed completed-frame median | 1,198.0 | 1,122.5 |
| Theed completed-frame p95 | 1,527.1 | 1,412.3 |
| Theed submission CPU median | 15.1 | 18.1 |
| Bespin completed-frame median | 527.6 | 477.1 |
| Bespin completed-frame p95 | 691.7 | 585.9 |
| Bespin submission CPU median | 9.9 | 9.3 |

Completed-frame medians improve 6.3% and 9.6%; p95 improves 7.5% and 15.3%.
Shared-machine variance is substantial: the prior paired run was about 16 to 17%
faster, so that larger result should not be treated as a reliable FPS promise.
Theed's CPU submission increases by about 3 ms with visibility-index updates;
CPU-bound hardware can therefore behave differently from this GPU-bound software
renderer. Profile actual client hardware before generalizing the FPS benefit.

The stable work counters make the mechanism and tradeoff explicit:

| Work / sampled frame | Theed before -> after | Bespin before -> after |
| --- | ---: | ---: |
| Submitted triangles, final frame | 589,047 -> 523,131 | 503,214 -> 443,258 |
| Draw calls, final frame | 372 -> 372 | 335 -> 335 |
| Clears per frame | 12 -> 6 | 10 -> 4 |
| GPU parameter reads / 16-frame sweep | 32 -> 2 | 16 -> 2 |
| Texture subuploads / sweep | 154 -> 125 | 16 -> 16 |
| Buffer subuploads / settled sweep | 656 -> 1,235 | 336 -> 588 |

Color culling trades additional private index storage and uploads for fewer
submitted triangles. It does not add geometry passes or lower visible detail.
The final-frame pixel hash, including the HUD, matches before/after in every
round on both maps. It is stable across the two final paired runs as well.
[Raw measurements and source hashes](render-performance-results.json) retain the
counters, timing summaries, and image hashes. This exercise freezes simulation;
it does not establish walking/combat frame pacing or eliminate first-use shader
compilation stalls for effects introduced later.

## GPU-side pass

The passes of `Renderer::battle_frame` (`rust/bfe-render`) at High with the
temporal upscaler: the cascade shadow passes, the far scene, the near scene
(with the soft-particle depth copy and the water's colour/depth copy when water
is in view), the player's first-person view, the motion-vector pass, the
quarter-size soft-focus copy, bloom (three quarter-size passes), the composite,
and the upscaler's motion, resolve and sharpen passes. FXAA runs only without
the temporal upscaler. What this pass changed is in [rendering.md](rendering.md)
("GPU-side pass"); this file keeps the measurements.

### Theed, High preset, 640x400, software renderer

| Metric, per frame (p50 unless noted) | Before | After |
| --- | ---: | ---: |
| World draws | 368 | 220 to 286 |
| World triangles | 692,000 | 342,000 to 385,000 |
| Shadow draws | 181 | 46 to 118 |
| Shadow triangles | 481,000 | 147,000 to 189,000 |
| GL calls | 3,257 | 2,600 to 3,015 |
| Completed frame, p50, uncontended | 779 ms | 484 ms |
| Completed frame, p95, uncontended | 923 ms | see below |

The ranges on the after side span runs: the cascade windows land at
different moments depending on the walk, and the dynamic pass count varies
with how many bots are in shadow range. The uncontended times are from the
first 300 frames of the initial after run and the whole before run; for the
rest of the runs another benchmark shared the eight cores (load average 15 to
30), which doubled every frame time on both sides. Treat the time as
"a third less, measured once cleanly" and the counters as the record.

### Terrain on Theed

Forcing the terrain material to one layer (`TERRAIN_LAYERS = 1`) took the
frame from 765 to 739 ms; hiding the ground altogether, to 706 ms (100
frames, same load). Theed's plaza is baked architecture, not the height
field, so the layer blend is a few percent of this map's frame. The
composited terrain tile cache belongs to the open maps and is left for a pass
that measures on one of them.

### Fidelity

Two runs of the same code differ at rest by about 2 percent of pixels over
8 levels (mean 1.0 to 1.2 levels of 255): the trees and flower beds sway on
the wall clock, and the temporal history lands on slightly different
phases. Against that floor, before versus after:

| Change | Pixels over 8 levels | Mean levels | Note |
| --- | ---: | ---: | --- |
| Same code, two runs (noise) | 2.1% | 1.17 | trees, flower beds |
| Hardware sampling, four fetches everywhere | 6.0% | 2.07 | facades edge-on to the sun 14 levels darker: rejected |
| Hardware sampling, exact sixteen taps | 1.95% | 0.71 | identical within noise |
| Adaptive four/sixteen (shipped) | 2.3% | 1.05 | facade 0.5 levels, pillar 0.4 |
| Cache on versus cache off | 1.3% | 1.01 | the cache is exact |
| Whole pass at rest | 3.2% | 1.14 | the soldier's shoulders 0.3% |

Per-region means on the frame at rest (before / after): the left facade
77.7 / 77.6, the pillar 97.7 / 97.7, the soldier 107.4 / 107.2, the open
ground 127.1 / 127.1.

## Reproduce in Rust

Run the renderer unit tests and headless GL benchmark from `rust/`:

```sh
cargo test --manifest-path rust/Cargo.toml --locked -p bfe-render
cargo run --manifest-path rust/Cargo.toml --release --locked -p glbench -- --help
```
