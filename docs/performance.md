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

## Old GPUs at full screen

On an old integrated GPU a battle can run well in a small window and badly
at full screen with every quality option at its lowest. Whatever scales with
the window's pixels, rather than with the 3D frame's, is the cause:

- The temporal upscaler (the default) runs its motion, resolve and sharpen
  passes at the window's size whatever the render scale, and keeps two
  16-bit float history buffers of the window's size (33 MB at 1080p). The
  spatial upscaler is one 13-read pass at the window's size.
- A driver Chromium blocks moves work to the CPU: the game drawn in software
  (SwiftShader), or the window composited in software with every frame read
  back from the GPU. Both cost by the window's pixels.
- The render scale stopped at 0.5, and adaptive resolution did nothing
  where the browser offers no GPU timer (Firefox, and drivers without timer
  queries). It is also off unless a preset or the player turns it on.

### Measurements

SwiftShader stands in for a fill-limited GPU: every pixel is costly, so the
passes that run at the window's size show plainly. Theed, a 1920x1080 window,
the Minimum preset's settings with the upscaler varied, 30 driven frames after
a 4 s warm-up, `tools/render-bench-cdp.mjs`. Completed-frame medians in
milliseconds, on a shared host (runs of the same settings differ by up to 15
percent): **not hardware frame times**.

| 3D frame | Upscaler | Frame (ms) |
| --- | --- | ---: |
| 1920x1080 | off | 340 |
| 960x540 | off | 150 to 194 |
| 960x540 | bilinear | 160 to 185 |
| 960x540 | spatial | 250 |
| 960x540 | temporal | 523 |
| 672x378 | off | 155 |
| 1440x810 (Performance preset) | temporal | 770 |
| 1440x810 (Performance preset) | spatial | 456 |
| 1440x810 (Performance preset) | bilinear | 370 |

Three interleaved rounds at 960x540 put off at 152 ms (median), bilinear at
171 and the far scene's target at half size at 154: halving the far scene,
which holds only the sky and the map past the near scene, isn't worth its
blur, and is left out.

At the window's size, the temporal upscaler costs more than the half-size
frame it rebuilds: the old Performance preset (0.75 and temporal) took more
than twice as long as the whole frame drawn at 1920x1080 with no upscaler.
What a real GPU gives depends on its fill rate and bandwidth, but the passes
are the same.

### Changes

- **Bilinear upscaler** (`upscaler: "bilinear"`, `src/upscaler.js`): the 3D
  frame stretched by the GPU's texture filter, one read a pixel, with the HUD
  drawn sharp over it. When nothing else is composited (no soft focus, bloom,
  motion blur or FXAA), the composite pass itself draws to the window, so the
  stretch costs no pass of its own. With FXAA, FXAA runs at the 3D frame's
  size and one bilinear pass stretches it.
- **Presets:** Performance (0.75) uses the bilinear upscaler instead of
  temporal, and keeps FXAA. Minimum keeps the browser's stretch (upscaler
  off), still the cheapest: drawing the composite and the HUD at the
  window's size made its frame about an eighth slower under SwiftShader, for
  a sharp HUD (the UPSCALER row offers it). Balanced and High quality are
  unchanged.
  A preset's values are set when it is picked, so players who picked one
  before keep their settings until they pick again.
- **Render scale** goes down to 0.25. Its row's note in VIDEO OPTIONS gives
  the 3D view's size and the window's (`3D VIEW 960X540, WINDOW 1920X1080`),
  and the UPSCALER row's note what each upscaler costs. Adaptive resolution
  stops at 0.25 too.
- **Adaptive resolution without a GPU timer** (`src/render-profiler.js`,
  `src/runtime/frame-pacing.js`): a fence follows each frame, after the HUD.
  Its status is read without waiting (the browser refreshes it between
  tasks), at every display refresh and as the next frame renders. A frame
  whose fence is still unsignaled when the next renders left the GPU behind:
  sustained, that lowers the resolution, as the timer's evidence does. A
  fence seen signaled within 70 percent of the frame budget after it was
  sent is headroom: sustained for 8 s, the resolution comes back up. A
  CPU-bound page or a slow display finishes its fences in time, so neither
  shrinks the image. The timer, where there is one, still decides. In
  Chromium with the timer hidden, a GPU-bound load at a 30 FPS target went
  from 1 to 0.5 in about 11 s and then held 30 FPS; with the load gone it
  came back up in steps. (Under SwiftShader a fence's result reaches the
  page about a refresh late, so headroom at a 60 FPS target on a 60 Hz
  display only shows on real GPUs.)
- **The GPU in use** (`src/gpu-info.js`): the QUALITY PRESET row's note names
  it. In the desktop app, `bfeDesktop.gpu()` adds Chromium's feature status
  and GPU list (`electron/gpu.js`, as `chrome://gpu` shows them). The note
  and the stats line warn when the game is drawn or composited in software
  (update the graphics driver), and the note when a laptop draws on its
  Intel GPU while an NVIDIA or AMD one idles. The stats line also shows the
  3D frame's size, which adaptive resolution moves.
- **Two GPUs:** on Windows the desktop app starts Chromium with
  `force_high_performance_gpu`, which asks for the discrete GPU of a laptop
  that has two (Electron's documented switch; untested here, as this host
  has one GPU). Windows' own per-program setting still applies.
- **Reads behind switches on Direct3D:** Chromium on Windows runs WebGL
  through ANGLE, and Direct3D's shader compiler moves texture reads with
  implicit gradients out of branches (or flattens the branch), so reads meant
  to be skipped still run. The far image's read in the near/far dissolve,
  the composite's soft focus, bloom and motion-blur reads, and the bump map
  with bump mapping off now use explicit levels or gradients, which keep them
  inside their branches. The images are unchanged: the render targets have
  one level, and the bump map's gradients are the ones the implicit read
  used. On Theed, with bump mapping, bloom and soft focus on and then off,
  no pixel of 256,000 differs, at rest or after driven frames. What it saves
  on Direct3D is unmeasured here (the test host has none).
- **Spawn screen:** shut or covered, its endless CSS loops stop instead of
  costing style work every frame of the battle.

### On a slow PC

1. VIDEO OPTIONS > QUALITY PRESET: MINIMUM. Its note names the GPU; if it
   warns about software drawing or compositing, update the graphics driver
   first: nothing else here comes close.
2. Lower RENDER SCALE until the frame rate holds; the note shows the 3D
   view's size. ADAPTIVE RESOLUTION set to 60 FPS trades sharpness for
   frame rate on its own (30 FPS, the presets' choice, steps in only below
   about 26).
3. A laptop with two GPUs: Windows' Settings > Display > Graphics can also
   set Battlefront Eternal to High performance.
4. The Steam overlay, when turned on, moves Chromium's GPU work into the main
   process without DirectComposition (docs/presence.md); on a slow PC, leave
   it off.

### Reproduce

`CHROME` names the Chromium binary when `/usr/bin/chromium` isn't it:

```sh
CHROME=/path/to/chrome W=1920 H=1080 FRAMES=30 WARM=4 LABEL=bilinear \
  SETTINGS='{"renderScale":0.5,"upscaler":"bilinear","antialiasing":"off","shadows":"off"}' \
  node tools/render-bench-cdp.mjs
```

The Minimum preset's other values (`src/settings.js`) go in `SETTINGS` too;
picking a preset in the menu sets them, but a stored `performancePreset`
alone does not. Interleave the configurations being compared: the host's
load moves every timing. To compare images, compare trees that create the
same GPU objects: the driver seeds `Math.random`, three's object IDs draw
from it, and one more material moves every flower bed after it.

## Old GPUs: vertex work, textures and memory

Measured on a 2011 desktop: Intel Core i5-2400 with HD Graphics 2000 (Sandy
Bridge GT1, six execution units), Mesa 22.3 (`crocus`), 4 GB of RAM, a
1920x1080 60 Hz display, Debian 12 with MATE, the desktop app at full screen.
Its owner's settings had already dropped everything to the floor: render
scale 0.5, the bilinear upscaler, FXAA, no shadows or effects. Adaptive
resolution then held about 25 FPS by shrinking the 3D view to 480x270, which
is what made the distance a blur. At the defaults the same battle ran at 4
FPS.

### What the GPU spent its time on

- **Not pixels.** Chromium's GPU process spent most of every frame waiting
  in `SwapBuffers` for the GPU, which `intel_gpu_top` showed 100% busy at
  its top clock. Yet the near scene took 19 ms at 480x270 and 22 ms at
  960x540: four times the pixels, an eighth more time. It was vertex work.
- **Every uniform read from memory.** Mesa on this generation places a
  shader's uniforms in a buffer and fetches each vec4 a vertex uses with a
  load of its own. The map's lit vertex shader was 657 instructions with
  61 such loads a vertex. 56 of its 75 uniform vec4s were three's struct
  array of the scene's 14 point lights (the command posts' six and the
  ordnance pool's eight, idle ones at intensity zero), looped over at every
  vertex. Hiding the lights alone took the near scene from 21 to 13 ms.
- **Triangles off screen.** The static world's culling drew one range from
  the first triangle in a visible cell to the last; with the buckets'
  triangles in their original order, that was nearly every triangle. Each
  off-screen vertex was still shaded before being clipped.
- **FXAA.** 7.6 ms at 960x540 (28 ms at 1080p): BF1's textures leave few
  pixels under its contrast threshold, so almost every one takes the edge
  search.
- **BC7 unpacked.** The driver offers BC7 but the GPU can't sample it:
  Mesa unpacked every texture to RGBA8 on upload.
- **The window-sized canvas.** At render scale 0.5, the bilinear upscaler
  (canvas at 1920x1080) took about 5 ms a frame more than the browser's
  stretch (canvas at 960x540): the stretch pass, the HUD and Chromium's own
  copies of the canvas all run at the canvas's size.
- **The desktop compositor.** MATE's compositing cost about 15% (27 to 31
  FPS with it off). Marco ignores `_NET_WM_BYPASS_COMPOSITOR`, so the only
  remedy there is turning compositing off.

### Changes

All but the last four leave the image as it was. Captures of the static
world at rest, compared with the build before, differ outside the foliage
(whose sway differs from one run to the next of the same build too) in
under 0.1% of pixels, by a level or two: the light vectors are now taken in
world space rather than view space.

- **Point lights** (`point-lights.js`, `materials.js`): each scene packs the
  lights that are on and in reach of its camera, and each draw loops only
  over those that reach what it covers: the visible cells of a map bucket
  (`render-culling.js`), a soldier's pose sphere (`actor.js`), an object's
  bounds otherwise. A light left out added nothing; the count three
  compiles the programs for still never changes. A light in the loop costs
  every vertex a load and a test even when it lights nothing there, which
  is why the list is per draw and not a mask.
- **The map's meshes in world space** (`world.js` `WORLD_SPACE`): their model
  matrix is the identity, so the shader takes the view matrix as the
  model-view matrix and skips the model and normal matrices.
- **One-bone skinning** (`actor.js` `RIGID_SKIN`): the soldiers' vertices
  follow one bone at full weight under an identity bind matrix; three's
  four-bone blend reduces to that one bone, term for term.
- **Static culling draws only visible runs** (`render-culling.js`), in one
  `WEBGL_multi_draw` call made with the state three has bound; the far
  scene's buckets are culled too. 162,000 triangles a frame became 104,000
  on the measured view.
- **Fewer passes:** FXAA over a plain frame reads the scene's image instead
  of a copy of it; FXAA reads its five-pixel cross before the diagonals.
- **FXAA (FAST)** (`fxaa.js`): FXAA 3.11's console path, 2.9 ms against 7.6
  at 960x540. The Performance preset uses it.
- **BC1/BC3 where BC7 is unpacked** (`textures.js`, `gpu-info.js`):
  `unpacksBc7` matches Mesa's names for Intel before Ivy Bridge. GPU memory
  for a map fell by about 80 MB and the near scene by 0.8 ms.
- **Audio:** on a PC with 4 GB or less, decoded stream audio is kept for two
  minutes of sound instead of ten (`audio/streams.js`); ten minutes of the
  game's music decoded is about 270 MB.
- **First session on an old GPU** (`settings.js` `adoptGpuPreset`,
  `gpu-info.js` `oldGpu`): Intel before Ivy Bridge, under Mesa or Windows'
  driver, starts on the Minimum preset instead of the defaults. Settings
  already stored stay as they are.

### Measurements

Theed, the same seeded battle and inputs, 270 frames driven one tick each
after a 15 s warm-up, the GPU kept busy (frames pipelined, as in play, which
also keeps it at its top clock), per-pass `TIME_ELAPSED` queries. The
owner's settings with the render scale fixed at 0.5 (960x540, bilinear,
FXAA). Milliseconds a frame:

| | Before | After (FXAA) | After (FXAA (FAST)) |
| --- | ---: | ---: | ---: |
| Wall time a frame | 46.1 | 32.8 | 28.7 |
| GPU, all passes | 39.3 | 27.1 | 22.8 |
| Near scene | 24.4 | 13.3 | 13.4 |
| Triangles | 161,792 | 103,671 | 103,671 |

Waiting for every frame instead lets the GPU drop to its idle clock between
frames; timings then move with the clock, not the work.

In play, at full screen with vsync on the 60 Hz display:

| Settings | Before | After |
| --- | --- | --- |
| The owner's (adaptive resolution at 30 FPS) | 25 FPS at 480x270 | 27 FPS at 720x405 to 840x473 |
| Minimum preset (adaptive resolution at 30 FPS) | 27 FPS at 720x405 | 37 FPS at 960x540 |

### What is left

- A frame that misses 16.7 ms waits for the next refresh, so this GPU holds
  30 FPS at best with vsync: the near scene's vertex work (10 ms at any
  size) and the window's passes don't fit in 16.7 ms. Turning vsync off in
  Chromium raised the average to 43 FPS but frames queued up behind the GPU
  (62 ms at the 90th percentile).
- The page allocates about 630 KB a frame (most of it in the battle's
  snapshot replication, iterators, raycasts and particles), so collections
  run often. The renderer's resident memory swings between 0.6 and 1.2 GB
  with them; four battles in a row in one page once grew it to 2.45 GB and
  the kernel killed it on this 4 GB PC.
