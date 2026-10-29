# Render and client-controls performance pass

This pass changes runtime rendering and input handling. It retains the existing
quality defaults, UI, model/import data, audio, leaf populations, light budget,
shadow resolution, draw order, and effects.

## Changes

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
- Particle and water captures obtain framebuffer handles from Three r170's
  existing target bookkeeping. This removes `FRAMEBUFFER_BINDING` reads inside
  a draw. The helper supports single-sample targets, explicit MSAA buffers, and
  multisampled render-to-texture. Recheck this adapter when upgrading Three.
- GPU timer queries remain asynchronous, with status polling reduced to the
  existing eight-frame sampling cadence. Disjoint samples are discarded.
- Full-screen post passes omit redundant clears; their triangles already
  overwrite every pixel with blending disabled. World, shadow, and HUD clear
  behavior is retained. Stable camera projections and first-person light-source
  storage are reused.
- Input resolves key/button bindings once and reads axes without temporary
  closures. Focus loss clears queued presses as well as held keys, preventing
  an old action from firing when the player returns.

WebGL queries can synchronize the browser and GPU process; see
[MDN's WebGL best practices](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API/WebGL_best_practices#avoid_blocking_api_calls_in_production).
There is no `finish`, pixel readback, or extra geometry pass in the production
changes. Synchronization and image capture below belong to benchmarks/tests.

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

Final code, SwiftShader Vulkan software renderer, 320×200, CP3 spawn, 4× MSAA
plus FXAA, low shadows, bloom/soft focus/enhanced water and foliage enabled,
including HUD rendering. Each map/version measures three alternating 16-frame
camera sweeps after warmup. Times below are milliseconds, including diagnostic
wrappers. They are **not hardware GPU timings or display-paced FPS**.

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

| Work / sampled frame | Theed before → after | Bespin before → after |
| --- | ---: | ---: |
| Submitted triangles, final frame | 589,047 → 523,131 | 503,214 → 443,258 |
| Draw calls, final frame | 372 → 372 | 335 → 335 |
| Clears per frame | 12 → 6 | 10 → 4 |
| GPU parameter reads / 16-frame sweep | 32 → 2 | 16 → 2 |
| Texture subuploads / sweep | 154 → 125 | 16 → 16 |
| Buffer subuploads / settled sweep | 656 → 1,235 | 336 → 588 |

Color culling trades additional private index storage and uploads for fewer
submitted triangles. It does not add geometry passes or lower visible detail.
The final-frame pixel hash, including the HUD, matches before/after in every
round on both maps. It is stable across the two final paired runs as well.
[Raw measurements and source hashes](render-performance-results.json) retain the
counters, timing summaries, and image hashes. This exercise freezes simulation;
it does not establish walking/combat frame pacing or eliminate first-use shader
compilation stalls for effects introduced later.

## Reproduce

Before editing, copy the current `src` directory somewhere with resolvable
dependencies. Then run:

```sh
RENDER_CPU_REFERENCE=/path/to/before/src node tools/benchmark-render-cpu.mjs
```

For whole-frame A/B measurement, serve two frozen project snapshots on separate
ports. They must have identical non-render code and assets, separate Vite
`cacheDir` paths, and differ only in the modules being tested. This matters
whenever other work changes the tree while a benchmark runs.

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs \
RENDER_BENCH_URLS=http://127.0.0.1:18457,http://127.0.0.1:18459 \
RENDER_BENCH_WIDTH=320 RENDER_BENCH_HEIGHT=200 RENDER_BENCH_FRAMES=16 \
RENDER_BENCH_OUTPUT=/path/to/render-results.json \
node tools/benchmark-render.mjs
```

By default, the browser benchmark uses seeded layouts, 640×400, 4× MSAA plus
FXAA, low shadows, enabled default effects, water reflections, and the HUD. Each version
warms up for 12 frames and measures three alternating 40-frame camera sweeps per
map. The simulation is frozen; these are render-workload measurements, not a
walking/combat simulation or display-paced FPS. Startup simulation ticks are
suppressed, the spawn is fixed, and Three UUID randomness has a separate seeded
stream so adding GPU resources does not redistribute the game world. `cpu`
includes submission of reflection, scene, underwater, and HUD passes. `completed` additionally forces
a one-pixel readback after every frame, preventing work from being hidden in
an asynchronous queue. Neither metric is GPU-only elapsed time; completed time
includes the readback round trip. A plain `gl.finish()` was insufficient to drain
Chromium's virtual GPU command queue in preliminary testing and is not used for these timings.

`RENDER_BENCH_FRAMES`, `RENDER_BENCH_ROUNDS`, `RENDER_BENCH_MAPS`,
`RENDER_BENCH_WIDTH`, and `RENDER_BENCH_HEIGHT` adjust the sample. The default
browser explicitly uses SwiftShader; set
`RENDER_BENCH_GPU=hardware` to allow Chromium's default GPU backend, and check
the reported renderer to confirm actual hardware acceleration. Timing is
informational, with no brittle pass/fail threshold on this shared machine.

## Validation

```sh
node --test tests/render-pipeline.test.js tests/tree-canopies.mjs tests/camera.test.js
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/rendering.mjs
npm run build
```

The focused Node suite passes 15 tests. It compares active canopy attributes
against the original equations through 80 camera/zoom/detail changes and lamp
selection against full-sort results through 100 positions. It also covers input
semantics, focus cleanup, framebuffer selection, unavailable/disjoint GPU
queries, and the existing canopy/camera regressions. Static color culling is
checked through 45 views and object transforms, including cache eviction: every
triangle intersecting the frustum survives, selected triangles retain their
original order, and the source indices stay unchanged.

All 135 browser rendering assertions pass in both snapshots. Saved soft-particle
images are byte-identical before and after, with MSAA both enabled and disabled.
The suite also exercises animated and hidden shadow casters, lighting changes,
view-model lights, shadows, bloom, water/refraction/underwater rendering, target
resizing, and WebGL errors. The production build passes with the existing large
bundle warning.

The jetpack volume browser regression also passes with soft particles disabled
and enabled: one required depth capture, zero foreground-wall leakage, and no
depth capture when idle.

## GPU-side pass

The passes of `Graphics.render()` as they stand at High with the temporal
upscaler: the cascade shadow passes, the far scene, the near scene (with the
soft-particle depth copy and the water's colour/depth copy when water is in
view), the first-person view, the motion-vector pass, the quarter-size
soft-focus copy, bloom (three quarter-size passes), the composite, and the
upscaler's motion, resolve and sharpen passes. FXAA runs only without the
temporal upscaler. What this pass changed is in `rendering.md` ("GPU-side
pass"); this file keeps the measurements and how to repeat them.

### Harness

Playwright is not installed on this host, so the driver is a Chrome DevTools
Protocol script (the pattern of `tools/profile-deep.mjs`): headless Chromium
with `--use-angle=swiftshader`, a Vite dev server per tree (the before tree
is a copy of `src/`, `index.html` and `vite.config.js` with `public/` and
`node_modules/` linked), a fixed 640x400 viewport, the settings written to
`localStorage` before load, `Math.random` seeded so flower beds repeat, the
game's own animation loop suppressed, and every `WebGL2RenderingContext`
method wrapped to count calls. Each run warms the battle up for 20 s of
ticks without drawing, captures the frame at rest (eight jittered frames at
dt 0, then `readPixels` of the canvas), drives 600 frames of walking,
strafing, turning and firing with one tick each, timing the frame's CPU
submission and its completion (a one-pixel `readPixels` after it), and
captures the end frame. Per `renderer.render` call it records draws and
triangles by scene and target. Captures are compared pixel by pixel.

### Theed, High preset, 640x400, SwiftShader

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
first 300 frames of the first after run and the whole before run; for the
rest of the runs another benchmark shared the eight cores (load average 15 to
30), which doubled every frame time on both sides. Treat the time as
"a third less, measured once cleanly" and the counters as the record.

The final paired run, both trees at once for 600 frames on a host with a
load average of 11 to 13 from other work: before p50 774 ms, p95 905 ms,
369 draws, 692,000 triangles, 181 shadow draws, 481,000 shadow triangles,
3,257 GL calls; after p50 662 ms, p95 747 ms, 223 draws, 395,000
triangles, 52 shadow draws, 204,000 shadow triangles, 2,671 GL calls, 20
static cascade passes against 2,149 dynamic ones over the run. Two
SwiftShaders sharing eight cores narrow the gap: under SwiftShader the
per-cascade depth copy is a 16 MB memcpy a frame each (on a GPU, a blit),
so the paired figure (14 percent p50, 17 percent p95) is the floor and the
uncontended one (38 percent) the ceiling of what this host can show. At
rest the paired captures differ by 2.25 percent of pixels over 8 levels,
mean 1.16: the noise floor.

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

### Reproduce

`tools/render-bench-cdp.mjs` is the driver (it serves the working tree
itself, or `BASE` names a server for another tree); `tools/pixel-diff.mjs`
compares two of its `.rgba` captures. Environment: `BASE`, `LABEL`,
`FRAMES`, `WARM`, `W`, `H`, `OUT`, `SETTINGS` (JSON merged over the
defaults), `PRE` (an expression evaluated in the page after warm-up, for
experiments such as forcing `TERRAIN_LAYERS` or
`__bf.graphics.cacheShadows = false`). For a before/after pair, copy `src/`,
`index.html` and `vite.config.js` of the before tree somewhere, link its
`public/` and `node_modules/`, serve it on another port, and run both at
once so they share the host's load. The unit tests of the pure logic:

```sh
node --test tests/shadow-cascades.test.js
node tools/render-bench-cdp.mjs
node tools/pixel-diff.mjs /path/to/out/before-rest.rgba /path/to/out/run-rest.rgba diff.png
```
