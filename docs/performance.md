# World CPU performance


Collision queries, soldier movement, world rounds and items require the Rust simulation module on the page, local world worker and dedicated server. Module failures stop startup or the query. The former `sim`, `rays` and `movement` query parameters and `BFE_SIM` / `BFE_RAYS` environment controls have been removed. Historical measurements below describe the builds measured at the time; compare current builds against recorded fingerprints with `world-hash-matrix.mjs --against`, rather than selecting an old implementation at runtime.

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
node --test tests/collision-queries.test.js tests/foliage-import.test.js tests/movement-continuity.test.js
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
- **First session on an old GPU** (`settings.js` `adoptHardwareDefaults`,
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

## Sound on slow CPUs

A 2014 laptop with an A8-6410 (four 2 GHz cores sharing 15 W with the GPU)
ran its page's main thread flat out and its world worker near its limit in
a battle, while Chromium's real-time audio thread took 31 to 43% of a core
for the original (Classic) sound, ahead of both. On stereo speakers the
original now plays through the light mix (`docs/audio.md`, "The light
mix"), using the Rust voice engine by default, with browser nodes as the
fallback. It has no reverb or limiter. The initial browser-node light mix
used a buffer source and two gains per voice without a worklet. Measured
for that initial implementation on a desktop Xeon in headless
Chromium (`docs/audio.md`, "Cost"), the page's audio threads in a battle
went from 24.5% to 16.7% of a core on Theed (13 voices heard) and from
22.5% to 11.6% on Kamino, whose reverb the full output ran; the engine's
update on the main thread from about 196 to 115 µs. These measurements
compare the browser-node light mix with the previous full output; see
`docs/audio.md`, "The voice engine", for the current Rust implementation.

A first session on a low-end CPU starts on Classic (`cpu-info.js`
`lowEndCpu`: four threads or fewer and 4 GB of memory or less, as Chromium
reports them); settings already stored stay as they are, and the AUDIO
page's SOUND row says Classic is much lighter there.

## The world step on a slow CPU

On a laptop with an AMD A8-6410 (four cores held near 1.9 GHz) the offline
world worker took 17.3 ms a tick on average against the 16.7 ms a 60 Hz tick
has, so a full Theed battle ran about 15% slow, and stopped for 100 to 200 ms
whenever the worker collected its garbage. The bot count, the tick rate and
every tick's outcome stay as they were; these changes only do the same work
with less:

- A hull's wall probes and hover springs keep nearby-triangle caches, as a
  soldier's body spheres do, the vehicle-only barriers included. They walked
  the building tree 60 to 80 times a tick on Theed, mostly finding nothing.
- A polygon gather no longer clears a Map on every call (clearing allocates
  a table); it searches its short list, and takes a Map past 16 polygons.
- A server world (the worker, `bfeserver`) no longer steps the chase camera
  of the headless game's own player, who has no body there.
- Units, droidekas, combats and bots' controllers declare every field in
  their constructors (as undefined, what they read before), so each keeps
  one layout: fields added later gave Theed's units six, and the code that
  walks the battle's units went megamorphic and kept deoptimizing.
- A unit's hit shapes, the crowd push and a soldier's weapon records stopped
  making garbage every tick; lists reused per query are popped empty
  (setting a length to 0 is a runtime call and drops the list's storage).
- three-mesh-bvh made three typed-array views over a tree's buffer for every
  cast: 550 a tick on Theed, 1,300 on Hoth. `src/mesh-bvh.js` makes each
  buffer's views once.

`tools/world-hash.mjs` fingerprints every tick of a scripted battle (the
replay parity state, every unit, hull and round in full precision, and the
published snapshot bytes). Before and after these changes the fingerprints
were identical for 2,400 ticks on ten maps, sides and seeds, and for 6,000
ticks on six more.

### Measurements

Node 22 on a shared Xeon, Theed unless named; the before column is the
branch's start. Timings there moved by up to 20% between identical runs, so
they are paired runs' medians and only a guide to the laptop.

| World step | Before | After |
| --- | ---: | ---: |
| Typed arrays made a tick | 551 (Hoth 1,300) | 2 (Hoth 0.4) |
| Garbage a tick | 608 KB | 415 KB |
| Scavenges in 1,800 ticks, publication included | 133 (Hoth 182) | 108 (Hoth 149) |
| Time a tick | 1 | 0.80 to 0.87 |

### What is left

- A weapon's numeric fields start as small integers and become fractions as
  it fires, and a soldier gains `dead` and `gatherCaches` after it spawns;
  each change of layout deoptimizes the code that read the old one once.
  Soldier fields are listed in `tests/network-replay-inventory.test.js`, so
  declaring those two waits for a replay review.
- Most of the step is now the Classic AI, the soldiers' own physics and the
  hulls' seats, each spread over many small functions.

### Hulls posed twice a tick

`Vehicles.update` poses every hull for the tick (`aimTick`: placed at the
tick's pose, its seats' aim turned and its fire points read) and then draws
it (`render`) unless the host sets `cosmetic` to false, which none does, so
the world worker and `bfeserver` pose each hull twice a tick. For most
hulls the second pose is not the first, and the world reads the second: a
hover hull's bob and wobble read its age after aimTick has advanced it, and
a walker's legs feel the ground and take their walk pose in render. Both
stay. For a craft in flight whose flight the host steps, with no walk pose
and no legs, render gives back exactly aimTick's pose (no bob, its gear at
its own, its seats from the angles aimTick just turned), so right after
aimTick it now only moves the barrels' kick again, which reads the clock.
A seat posed with no time (dt 0) also no longer works out where each aimer
would turn, which only a turn reads. On Bespin Platforms, all of whose 20
hulls are craft, render took 9.4% of the world step under Node and takes
0.3%; Dune Sea and Hoth have four craft each, Naboo Plains none.
`tools/world-hash.mjs` gave the same fingerprints as before for every tick
of eight battles with and without the collision's WebAssembly.

On Bespin Platforms with the collision in JavaScript (`rays=js`; builds
before and after, the player on foot, 40-second recordings interleaved, two
of each):

| | Emulator, before | after | Laptop, before | after |
| --- | ---: | ---: | ---: | ---: |
| Recordings | 2 | 2 | 2 | 2 |
| World step, mean ms | 20.1 | 20.0 | 22.5 | 19.7 |
| Median, ms | 19.8 | 19.6 | 21.6 | 18.8 |
| 90th percentile, ms | 27.6 | 27.5 | 27.2 | 24.5 |
| Ticks a second | 48.2 | 48.3 | 43.5 | 49.5 |
| Gaps between ticks over 40 ms | 23.5 | 33.0 | 31.5 | 21.5 |
| Frames the playout clock waited | 330 | 319 | 436 | 294 |
| Frames a second (page) | 32.9 | 30.4 | 29.2 | 28.4 |

On the laptop the step fell by an eighth and the world kept up better; on
the emulated laptop the two pairs did not separate.

### When the world's ticks run

On the same laptop the world also stood still longer than its steps took.
Its worker slept until the next tick counted from when its clock read the
time, before the steps it then ran: each tick started a step's length late, and the tick
after a long step, due while it ran, waited up to a whole tick more, so a
27 ms step showed the page a 40 ms gap. Behind, it rested 4 ms between
catch-up batches, as a chain of `setTimeout(0)` calls is held to 4 ms. The
world fell behind until its clock dropped ticks: it ran 59.0 to 59.8 ticks
a second in a Theed battle on the emulated laptop. `src/runtime/local-world-pump.js`
counts the wait from when the steps finish, rounded up to whole
milliseconds, and runs a tick already due as soon as the page's queued
commands have, through a MessageChannel. The same ticks run with the same
clock limits; only their timing changes.

An explosion also asked, for every soldier on the field, whether terrain or
a building shielded him (a ray through the map) before it looked at his
distance: in 40 seconds of a Theed battle 71 explosions cast 2,619 of those
rays, 4 of them for soldiers in reach. `Combat.explode` now makes the sight
test only when the blast would push him, hurt him or show him where it came
from. The fingerprints were identical for 2,400 ticks on eleven maps, sides
and seeds, and for 6,000 ticks on six more.

Measured on a desktop emulating the laptop (each renderer thread in a cgroup
at the laptop's speed, the page's WebGL draw calls made no-ops), Theed,
medium AI, on foot; three interleaved pairs of 40-second recordings, means:

| World | Before | After |
| --- | ---: | ---: |
| Gaps between ticks over 40 ms | 10.3 | 3.7 |
| The same, after a recording's first second | 10.0 | 1.3 |
| Ticks a second | 59.65 | 59.94 |
| The page's input lead, 90th percentile (ticks) | 9.7 | 5.6 |
| Frames the page's playout clock waited on the world | 53 | 40 |

The first second of each recording holds a few long steps in both builds:
the recorder attaching to the worker. Step times varied more with each
battle than between builds (p50 8.0 to 11.8 ms, p90 14.0 to 18.5 ms over
the six recordings).

What still stops the world for over 40 ms there is mostly its own garbage
collection and the heaviest fights' steps. A full collection's final pause
took 10 to 31 ms, one or two in each recording, and the 25 to 28 young
collections 5 to 18 ms each, mostly between ticks.

### What the worker's full collections took

A full collection comes when the old generation has grown past a limit set
from what the last one kept (about half again, in these traces), so it
comes as often as the world fills the old generation. In a Theed battle on the emulated laptop the world worker's old
generation took on about 20 KB a tick, little of it data the world keeps:

- V8 makes a Map's or Set's new table (when it grows, or is cleared) in the
  generation its current table is in, and a Map that lives as long as the
  world is moved to the old generation by the first full collection. Each
  table such a Map was cleared or grown to every tick was garbage only the
  next full collection took: the collision world's polygon lookup (several
  gathers a tick grow it to 64 entries), the hit history's Map for each
  frame, each hover spring's Set and each command post's counts. The polygon
  lookup is now open addressing over typed arrays (`PolygonIndex`, emptied
  by a stamp); the hit history finds a body's record in the next frame by
  its place in the battle's list; a spring keeps the three polygons it met
  in a list; a post clears its counts only when it has some.
- The hit history cut its records to the bodies alive each capture and made
  new ones when more came; its frames are now pools with counts.
- The snapshot host keeps 128 snapshots as baselines, so every new entry a
  capture made (each moving soldier's, every publication) outlived the young
  generation. When the ring lets a snapshot go, the entries, positions and
  velocities that its successor does not hold at the same slot are held by
  none (a capture reuses only the previous snapshot's values, slot by slot),
  and `EntrySpare` hands them to the next captures to fill.

Headless, under Node 22 with the young generation sized as the worker's
(a young collection every 90 to 95 ticks), a full collection forced mid-
battle (so long-lived Maps are old, as they are after the worker's first)
and acknowledgements as a page sends them, the old generation of a Theed
battle grew 34 KB a tick; the hit history saved 5.0 of it, the polygon
lookup 6.7, the springs and posts 3.0, the snapshot entries 5.0, and 14
were left (pointers there are twice the worker's size). On the emulated
laptop, four interleaved pairs of 60-second recordings (Theed, medium AI,
on foot):

| World worker | Before | After |
| --- | ---: | ---: |
| Old generation growth, KB a tick | 19.9 | 11.3 |
| Full collections in the 4 minutes | 5 | 2 |
| Their final pauses, ms | 25, 27, 37, 48, 51 | 23, 31 |
| Young collections' time a minute, ms | 394 | 315 |

Gaps over 40 ms between ticks and the page's playout waits moved less than
the battles did. Young collections came as often as before: what the world
allocates is unchanged, only what outlives them.

## The collision's rays in WebAssembly

A profile of the world worker on the emulated laptop finds no single hot spot
in its step. Sorted by what moving each part of it to another language would
take (an unminified build with the profiler on, 30 seconds of a battle each,
medium AI, on foot; ms of each tick's server step, publication included):

| Part of the step | Naboo Plains | Dune Sea | Bespin Platforms |
| --- | ---: | ---: | ---: |
| The collision's rays (cast, castMoving, ground) | 1.59 (14%) | 4.20 (18%) | 5.62 (23%) |
| Sphere gathers and the contact searches over them | 1.18 (11%) | 1.64 (7%) | 0.86 (3%) |
| Rays against soldiers' hit shapes | 0.60 (5%) | 1.24 (5%) | 0.50 (2%) |
| Snapshot byte encoding | 0.34 (3%) | 0.36 (2%) | 0.33 (1%) |
| three.js transforms under game objects (rigs, seats, poses) | 1.54 (14%) | 3.83 (16%) | 4.36 (18%) |
| Game logic over objects | 5.96 (53%) | 12.63 (53%) | 13.15 (53%) |
| The whole step | 11.22 | 23.89 | 24.83 |

The first four read and write only numbers: each could move behind an
interface of a few typed arrays. The transforms are numbers too, but they
belong to three.js object trees that game code walks and changes, so moving
them means moving those trees. The rest (the AI, weapons, hulls' and seats'
logic, the battle, snapshot capture) is logic over the battle's objects.

The rays are the largest of the four, and they come from all over the step:
a flyer's sweep and landing probes, a seat's aim, line-of-sight tests, a
soldier's steps and ground, rounds in flight. The collision module of
`rust/sim` (once the crate `rust/rays`) computes them, and the sphere queries
after them, in Rust compiled to WebAssembly: in the
offline world worker unless the battle's query has `rays=js`, and on a page
or a dedicated server when asked (`rays=wasm`, `BFE_RAYS=wasm`).

### What it computes, and how it is loaded

The module does what `cast`, `castMoving` and the ground's ray (`ground`) do
in collision.js: three-mesh-bvh's walk of a tree to its first hit (indirect,
double-sided) through the buildings, each body and the terrain; the terrain
grid's walk; and cast's loop over the bodies, each moving one's ray taken into
its frame and its hit brought back. It keeps copies of what those read. Trees
never change once built, so each is copied once: three-mesh-bvh's packed
nodes, the corners (as f32 where JavaScript holds a Float32Array, widened as
it widens them), the index and indirect buffers, and the grid's cells. A
moving body's bounds and matrices are copied as it moves. Props, traps and
hulls switch bodies on and off by setting `enabled`, a plain field, so a cast
first asks the module which bodies its ray can meet, in the bodies' order,
and tells it which of those are enabled. A cast writes its ray to eight f64
slots in the module's memory and makes those two calls; the hit comes back in
seven slots and a code (no hit, terrain, the buildings, or a body's slot).

Two first designs cost more than they saved. An accessor on each body that
told the module as `enabled` changed gave each body object a hidden class of
its own, and the JavaScript loops over bodies slowed down (a gather took 1.8
times as long in one probe). Copying every body's `enabled` before every cast
cost 0.56 ms a tick on Bespin Platforms, a quarter of what the rays still
took there.

`tools/build-rust.mjs` builds the crate (`cargo build --release --target
wasm32-unknown-unknown -p sim`; no wasm-bindgen, as the module imports
nothing) and writes `src/sim-wasm.js`, the module as base64 (52 KB of
WebAssembly when it held the rays alone; 141 KB, 199 KB as base64 and 73 KB
compressed, now that the simulation's module holds the movement, the pose
sampler and the entity store too, docs/sim-store.md). That file is
committed, so building the game needs no Rust. game.js imports
`src/sim/module.js` only for a battle that asks for one of its kernels, so
Vite makes it a chunk of its own that the page and the world worker share,
and the default bundle loads nothing more. The world
worker, the page and plain Node (the dedicated server, which copies `src/` as
it is; `tools/world-hash.mjs`) all load it the same way, with
`WebAssembly.compile` and `instantiate`: there is no wasm file to bundle,
serve from the app's archive, or stub out under Node. The desktop app's
content policy already allows `wasm-unsafe-eval`. The module is compiled once
per thread (about 10 ms under Node), and each battle gets an instance of its
own, whose copies (3 to 12 MB a map, made in 7 to 20 ms) go with it when the
battle ends.

### The same answers

Each function repeats the JavaScript it stands for operation by operation, in
the same order, in f64: three.js's Ray, Vector3 and Triangle methods,
three-mesh-bvh's traversal, and collision.js's grid walk and body loop. None
uses a function whose last bit could differ from V8's: there is no sine,
cosine, exponential or power, and square roots and floors are exact in both.
The one such value a cast needs, `Math.hypot` of its direction (which decides
between the terrain's grid and its tree), comes in from JavaScript.
`Math.min` and `Math.max` keep JavaScript's rules for NaN and signed zeros.
WebAssembly has no fused multiply-add unless asked for, and Rust never
contracts f64 operations by itself.

- `tests/ray-kernel.test.js` compares every cast, castMoving and ground probe
  with the JavaScript's, field by field with `Object.is`, the body by
  identity, on a made-up world whose bodies switch on and off, move, come and
  go.
- `tools/check-ray-kernel.mjs` does the same in a running battle on every map,
  with gathers, overlaps and terrainNear too, and also answers every query the
  battle itself makes and runs every hull's wall probes both ways, from its
  loading on: 168,000 answers of its own on each of the 17 maps, 170,000 to
  560,000 of the battle's queries a map and 6,000 to 22,000 wall-probe runs
  on each map with hulls, every one the same.
- `tools/world-hash.mjs --rays wasm` fingerprints a battle cast this way.
  The branch's start, this branch without `rays=wasm`, and this branch with
  it gave the same fingerprint for every tick: 2,400 ticks on 24 maps, sides,
  seeds and modes (Naboo Plains, Dune Sea, Bespin Platforms and Hoth in
  several of each, seven other maps, acknowledgements, a page's interval and
  shots judged rewound), and 6,000 to 9,000 ticks on seven more. With the
  sphere queries, the broad phase and the wall probes in the module too, the
  same 31 battles gave the same fingerprints without and with it, and without
  it the same as before these changes; six of them gave the same
  fingerprints in all four ways the collision's module and the soldiers'
  movement's can be on or off.

### Sphere queries

The queries soldiers, hulls and droidekas make about a sphere run in the
module too: `gather` (the polygons whose triangles' bounds meet the
sphere's, each with those triangles), its cached form, `overlapsSphere` and
`terrainNear`. `rust/sim/src/collision/spheres.rs` repeats collision.js and
three-mesh-bvh's shapecast: the same walk of a tree (a child's bounds tested
before going down to it, never the root's; a leaf's triangles in their
indirect order), the same bounds tests written the same way (`a < b` and
`!(a >= b)` part ways when a number is NaN), each caller's cache filled and
kept as collision.js keeps it, and a moving body's box and triangles taken
through its matrices. The polygons come out in the same order with the same
triangles, to the last bit. The module keeps each body's polygon numbers and
offset, and the buildings and the vehicle-only barriers as fixed sets with
each triangle's bounds.

A JavaScript caller makes two calls, as a cast does: the first does the
buildings and the barriers and names the bodies the sphere can meet;
JavaScript writes whether each is enabled; the second does those bodies and
writes the polygons out (each one's number and whether it moves, and each
triangle: a fixed set's by its number, a body's by its nine corners).
collision.js fills its pooled lists from that, a fixed set's triangles being
its own Triangles as before, so the callers' contact searches see what they
saw. The same functions are public in the crate (`gather`,
`overlaps_sphere`, `ground`, `terrain_near`, `closest_point_to_point`) for
other kernel modules, such as the soldiers' movement, which pass every body's
flag by its place in the world's order and own their caches and lists, all
within one call.

A hull's wall probes (`Vehicle.pushWalls`) run whole in the module: both
passes over the probes, each placed by the hull's turn, its gather through
its cache and the terrain's triangles near it, and the push out of each wall
triangle (`pushOut`, operation by operation as vehicles.js writes it, a
walker's probes giving way to another hull only as far as they came).
JavaScript writes the probes, the hull's place, velocity and turns and each
body's flag once, and gets back the hull's place and velocity and whether
its turn is undone: no polygon or triangle list comes back to be walked
once. On Dune Sea, with a dozen hover hulls, pushWalls took 4.1% of the
world step under Node with the gathers in the module, a third of it copying
the terrain's triangles out, and takes 2.5%.

### The bodies a cast tries

A cast tries, in the world's order, every body that is not left out and
whose bounds its ray meets; the ray's test ignores how far the cast reaches,
so every body's bounds were tested for every cast. Bespin Platforms has 96
bodies, four casts in five there reach less than 5 m (0.9 m at the median),
and on average a tenth of a body passed: the walk was 4% of the world step.
`rust/sim/src/collision/broad.rs` files the fixed bodies in a grid over x and z, made
again when the world's order or a fixed body's bounds change. A cast with a
finite reach whose segment covers at most 16 cells takes the fixed bodies
filed there whose bounds come within a millimetre of the segment's, and
every moving body, and gives each the walk's own tests in the world's order.
A fixed body it leaves out lies wholly clear of the segment, by far more
than any rounding, so none of its triangles can be hit within the cast's
reach and it could never have changed an answer. Moving bodies are cast
against in their own frames and keep the ray's test, and longer casts, and
casts with no finite reach, test every body as before. Under Node the walk's
share of Bespin Platforms' step fell from 4.0% to 1.3%.

The module's memory grows as it makes these structures, and growing it
empties every typed-array view made over it before: each answer is read
through a view made after the calls before it. `tests/ray-kernel.test.js`
makes every call grow the memory and compares every kind of query with the
JavaScript's.

### Measurements

With the casts alone (the first module), on the emulated laptop (one build,
with and without `rays=wasm`; 40-second recordings of a battle, medium AI,
on foot, interleaved: without, with, without, with; means of each):

| | Naboo Plains, JS | WASM | Dune Sea, JS | WASM | Bespin Platforms, JS | WASM |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Recordings | 3 | 3 | 3 | 3 | 3 | 4 |
| Server step, mean ms | 10.9 | 9.8 | 21.3 | 19.9 | 25.7 | 20.9 |
| Median, ms | 9.9 | 8.8 | 20.5 | 19.2 | 24.8 | 20.1 |
| 90th percentile, ms | 16.5 | 14.9 | 29.9 | 28.1 | 35.7 | 29.8 |
| Ticks a second | 60.0 | 60.0 | 45.7 | 48.9 | 38.2 | 46.4 |
| Gaps between ticks over 40 ms | 3.7 | 3.3 | 29.7 | 17.3 | 75.7 | 32.0 |
| Frames the playout clock waited | 43 | 46 | 246 | 227 | 354 | 252 |

With the module every Bespin Platforms recording's step was shorter than
every one's without it, and on Naboo Plains each pair's was (the first by a
tenth); on Dune Sea two pairs' of three were, and the third's was 0.3 ms
longer. On Dune Sea and Bespin Platforms the world cannot keep 60 ticks a
second here with or without it, but it falls behind less often and less far.

In the profiles above, run again with the module, the rays took 0.68 ms a
tick instead of 1.59 on Naboo Plains, 1.74 instead of 4.20 on Dune Sea, and
2.72 instead of 5.62 on Bespin Platforms, where half of what is left is
cast's test of every body's bounds (96 of them) for every ray (the broad
phase above cuts that). The rest of the step stays as it was, give or
take what one battle differs from another. The module makes 2 to 4% less
garbage than the JavaScript did (it made a few objects for every triangle a
ray met), so the worker's collections stay as they were: the gaps it saved
on Dune Sea and Bespin Platforms are long steps'.

Under Node 22 on the development machine (`tools/world-hash.mjs`, 2,400 ticks,
mean world step after tick 900, the two alternated over four rounds):

| World step after tick 900, ms | JavaScript | WebAssembly | Change |
| --- | ---: | ---: | ---: |
| Naboo Plains | 6.32 (5.56 to 7.28) | 5.92 (5.51 to 6.22) | -6% |
| Dune Sea | 11.98 (11.48 to 12.35) | 10.43 (10.21 to 10.71) | -13% |
| Bespin Platforms | 12.18 (11.45 to 12.95) | 11.17 (10.24 to 11.86) | -8% |

Other work shared the machine, so runs of the same build differ by up to 30%:
read these as a guide, not to the last tenth.

Each call alone, in a battle's world 600 ticks in (best of ten passes over
20,000 random rays through the map, under Node), a cast took 1.9 to 2.2
times as long in JavaScript as in the module (7.6 against 3.5 µs on Naboo
Plains, 4.5 against 2.3 on Dune Sea, 5.4 against 2.5 on Bespin Platforms),
and a ground probe 1.6 to 1.7 times as long (1.1 against 0.6 µs).

With the sphere queries and the broad phase too (one build, before the
hulls' wall probes moved in, with `rays=js` and with `rays=wasm`, which
turns the module on in the page as well as in the world worker; the player
spawned and on foot, medium AI, 40-second recordings interleaved, two of
each; the hulls posed once a tick in both), on the emulated laptop:

| | Naboo Plains, JS | WASM | Dune Sea, JS | WASM | Bespin Platforms, JS | WASM |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Recordings | 2 | 2 | 2 | 2 | 2 | 2 |
| World step, mean ms | 11.9 | 10.2 | 21.1 | 18.1 | 18.7 | 15.3 |
| Median, ms | 11.0 | 9.3 | 20.6 | 17.2 | 17.9 | 15.1 |
| 90th percentile, ms | 18.2 | 15.5 | 30.4 | 26.3 | 26.8 | 21.5 |
| Ticks a second | 59.8 | 60.0 | 45.6 | 52.6 | 50.9 | 57.9 |
| Gaps between ticks over 40 ms | 5.5 | 1.0 | 34.5 | 20.0 | 21.0 | 12.5 |
| Frames the playout clock waited | 52 | 48 | 359 | 220 | 279 | 101 |
| Frames a second (page) | 27.5 | 27.6 | 26.8 | 25.5 | 30.4 | 30.9 |

On the laptop itself (the same build and recordings; Dune Sea one of each):

| | Naboo Plains, JS | WASM | Dune Sea, JS | WASM | Bespin Platforms, JS | WASM |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Recordings | 2 | 2 | 1 | 1 | 2 | 2 |
| World step, mean ms | 12.6 | 11.8 | 22.6 | 19.9 | 20.0 | 16.7 |
| Median, ms | 12.0 | 10.9 | 21.8 | 19.2 | 19.2 | 15.9 |
| 90th percentile, ms | 16.6 | 16.1 | 27.5 | 24.2 | 24.3 | 21.1 |
| Ticks a second | 60.0 | 60.0 | 43.3 | 49.1 | 49.0 | 57.2 |
| Gaps between ticks over 40 ms | 5.5 | 5.5 | 33.0 | 12.0 | 16.0 | 15.0 |
| Frames the playout clock waited | 25 | 28 | 422 | 310 | 312 | 107 |
| Frames a second (page) | 29.6 | 28.5 | 23.2 | 22.1 | 28.2 | 26.8 |

The world's step is 9 to 17% shorter at the median and 3 to 20% at the 90th
percentile, and where the world could not keep 60 ticks a second it falls
behind less often and less far (Bespin Platforms runs 57 or 58 ticks a second
instead of 49 to 51, Dune Sea 49 to 53 instead of 43 to 46). The page drew 1
to 1.5 frames a second fewer with the module on the laptop, on each map, and
on the emulated laptop about the same on Naboo Plains and Bespin Platforms
and 1.3 fewer on Dune Sea, where the world ran 15% more ticks a second for it
to show.

A query's `rays=wasm` also gives the page a module of its own, through which
its collision world answers: about 9 to 12 casts, 10 to 13 ground probes and
10 to 13 gathers a frame, nearly all from replaying his own soldier. In one
battle on the emulated laptop with only the page's queries switched between
the module and the JavaScript every 5 seconds (16 periods, about 1,230 frames
each), the page's simulation phase was within a quarter of a millisecond
either way (its gathers come back to JavaScript as lists for one soldier,
no cheaper than the JavaScript's cache), and its render phase was 0.4 to
0.5 ms slower at the median with the module (12.68 against 12.13 ms on Naboo
Plains, 8.15 against 7.73 on Bespin Platforms, whose mean was 10.21 against
8.61 with a heavier tail), though that phase makes no collision query in
these settings and the module's memory lies outside the JavaScript heap.
Why is not known yet; it matters before the page's replay runs in the
module. The page therefore keeps the JavaScript unless its query asks.

As shipped (the offline world worker with the collision's module and the
soldiers' movement in it by default, the page in JavaScript), against
everything in JavaScript (`rays=js&movement=js`), on the emulated laptop
(one build, the same recordings):

| | Naboo Plains, JS | default | Dune Sea, JS | default | Bespin Platforms, JS | default | Hoth, JS | default |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Recordings | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| World step, mean ms | 9.8 | 9.2 | 23.7 | 16.6 | 18.9 | 14.5 | 17.9 | 14.9 |
| Median, ms | 8.8 | 8.2 | 23.4 | 16.1 | 18.1 | 13.8 | 16.9 | 14.3 |
| 90th percentile, ms | 15.1 | 14.6 | 33.4 | 23.3 | 26.6 | 20.6 | 25.8 | 21.6 |
| Ticks a second | 60.0 | 60.0 | 41.2 | 55.6 | 51.0 | 58.8 | 53.2 | 58.1 |
| Gaps between ticks over 40 ms | 4.0 | 2.5 | 49.5 | 13.0 | 30.5 | 16.5 | 16.5 | 11.5 |
| Frames the playout clock waited | 41 | 29 | 425 | 164 | 262 | 76 | 209 | 88 |
| Frames a second (page) | 26.2 | 26.7 | 26.2 | 27.0 | 31.3 | 31.1 | 26.7 | 26.3 |

The world's step is 7 to 31% shorter at the median (Dune Sea now keeps 56
ticks a second instead of 41, Bespin Platforms 59 instead of 51, Hoth 58
instead of 53), and the page, in JavaScript either way, drew the same frames
a second within 0.8.

### On a dedicated server

bfeserver runs the same world in Node: its world thread calls the engine's
`createGame` with the battle's query, and the engine loads the module there
as it does in a page's worker, from `src/sim-wasm.js`, which the host
runtime copies with the rest of `src/` (compiled once a thread in about
10 ms; each battle's own instance and copies, 3 to 12 MB a map, made in 7 to
20 ms as it loads). A server opts in with `BFE_RAYS=wasm` in its
environment: under Node the engine takes it when the battle's query says
neither `rays=wasm` nor `rays=js` (a query's `rays` would also reach the
players, whose pages load the battle's query). Nothing in the server
changes, and its default stays the JavaScript.

If the module cannot be compiled or instantiated (no WebAssembly, no memory
for it), the battle loads with the JavaScript and the server logs it once. If
a query fails inside the module during a battle (a fault in it, or memory it
cannot grow), the collision world drops the module and answers that query
and every later one in JavaScript, and logs it once.

Neither changes a tick. The module answers each query as the JavaScript
does, to the last bit, so a tick's outcome does not depend on which one
answered: a server using the module and pages that do not (or the other way
round) compute the same world, and a page's replay of its own player against
its own collision world makes the same corrections either way. The proofs
are the ones above, run on both paths in the same process:
`tests/ray-kernel.test.js` (and a query failing in the module half way
through a battle), `tools/check-ray-kernel.mjs` on every map, and
`tools/world-hash.mjs`, which forces one path or the other (`--rays wasm`,
else `rays=js`) whatever the environment says.

### What porting more would take

The rays kept their answers and lost a little over half their time.
WebAssembly walks a tree 2.1 to 2.8 times as fast as three-mesh-bvh does,
and only part of that is the layout: the same walk written in JavaScript over
the same flat arrays (no allocation, one reciprocal a ray, results in a typed
array) is 1.1 to 1.6 times as fast as three-mesh-bvh, and WebAssembly is 1.8
to 2.1 times as fast as that. A call into the module costs about 4 ns under
Node and 30 ns in the emulated worker (60 ns with a cast's slot writes and
reads), and a step makes 100 to 700 of them, well under a tenth of a
millisecond: crossings do not limit a kernel like this one, though they would
limit one called for every triangle.

With the sphere queries, the broad phase and the hulls' wall probes the
module answers every query the step makes of the collision world; the hover
springs' and the droidekas' contact searches over their gathers stay in
JavaScript, the springs because each contact feeds the hull's physics before
the next is searched. Of the parts above, two more read only numbers, and
neither pays as a kernel:

- Rays against soldiers' hit shapes (`Units.raycast`, and the hit history's
  for shots judged rewound) are 2.5 to 3.7% of the step under Node, but
  little of that is the rays. A unit's hit shapes are made when something
  first asks for them in a tick (`unit.sphere`), from its pose with V8's
  sines and cosines: 1.6% of the step on Naboo Plains and Dune Sea, which
  stays in JavaScript, and when it happens is part of the battle's outcome
  (a unit asked for before it moves has the shapes of where it was). The
  segment tests are 0.1 to 0.2% of the step; the rest is the loop over the
  battle's units, whose filters are JavaScript callbacks.
- Snapshot byte encoding is 5% of the step under Node and 1 to 3% on the
  emulated laptop. The encoder writes each entity's record against its
  baseline from JavaScript objects (each class's groups of fields, Maps of
  slots and world fields), so a kernel would first need every entry copied
  into its memory for every publication: the same walk over the same objects
  the encoder makes now, leaving only the byte writes to gain.

The rest, about 70% of the step, is the transforms and the logic over
objects: the AI, weapons, hulls, seats and the battle, some 60,000 lines of
the worker's engine code, with the three.js object trees they pose. Moving it
would take the worker's garbage collections out of the step too, but the
rays' gain says little about what it would get, as they were the most
array-like part of the step. Each of its sines, arc tangents, powers and
hypots would need V8's last bits; its iteration and sort orders would have to
be kept; and the page's prediction, which runs the same soldier and hull code,
would need the same module.

## The battle renderer in Rust

By default, `rust/bfe-render` (Rust compiled to WebAssembly, built by
`npm run rust:web` into `src/rust-pkg/`) draws part of the near scene inside
three.js's own pass, on the same WebGL2 context: the map's opaque buckets
(scrolling ones, the banners' wind and the leaves the tree canopies take
over included) and the ground, then every visible soldier's body and the
guns in his hand, instanced, then what the near scene blends: the map's
blended buckets and the command posts' holograms and flares, in three's
order. Its programs are made from the GLSL three compiled for each
material, and vehicles' and props' skinned rigs (`?rustRigs=0` leaves
those to three, for comparisons), and the sun's shadow cascades. Three.js
draws unsupported items and takes over if Rust fails to load or the WebGL
context is lost. Enhanced features use the same per-item fallback. `?rust`
remains accepted for old links and has no effect. `?rustOff` skips loading
the Rust renderer and its WebAssembly entirely, for recovery or comparisons.

`?rustSort` orders the map's buckets by program rather than as three does,
and `?rustRecord` keeps every call made to it, so that `rust/bfe-native` can
replay the scene natively.

### Passes and phases

The renderer is made of passes, a module each on both sides of the
boundary, so that each part of the frame can move to it on its own:

| Pass | Rust (`rust/bfe-render/src`) | Page (`src/rust-render`) | Phase | Draws |
| --- | --- | --- | --- | --- |
| World | `world.rs` | `world.js` | near | The map's opaque buckets, those with shader hooks included |
| Instances | `instances.rs` | `instances.js` | near | Soldiers' bodies, held guns, rigs, flying models |
| Far | `far.rs` | `far.js` | far | The far scene: sky and its layers, low-detail buckets, far terrain, far water, the sky's flyers; the same from a water mirror's camera and for the sky probes' faces |
| Terrain | `terrain.rs` | `terrain.js` | near | The ground and its painted layers (drawn by the world pass) |
| Transparent | `transparent.rs` | `transparent.js` | transparent | What the near scene blends, in three's order: blended buckets, holograms, flares |
| Effects | `effects.rs` | `effects.js` | transparent | Particle batches, Classic lightning, muzzle flashes, shields, haze, snow, rain, the Enhanced rain, steam, fountain and wake effects, the Enhanced arcs, the droideka's dust, the jetpack's flame and wake, the destruction's fire, smoke and arcs, the droideka's aura and the death camera's trace |
| Foliage | `foliage.rs` | `foliage.js` | near | Placed foliage, instanced, its patches chosen there too |
| View | `view.rs` | `view.js` | view | The first-person arms and weapon, and the muzzle flash |
| Post | `post.rs` | `post.js` | post | Soft focus, bloom, composite, FXAA, the upscale (bilinear, spatial, temporal) |
| HUD | `hud.rs` | `hud.js` | hud | The battle HUD's quads |
| Water | `water.rs` | `water.js` | transparent | Terrain water among the blended items: Low's textured surface, High and Ultra's tint and reflection passes, the rings, the shore's lapping, the Enhanced ocean's crest spray |
| Shadow | `shadow.rs` | `shadow.js` | shadow | The sun's shadow cascades: each cascade's static cache and live map, its casters drawn with three's depth programs |
| Motion | `motion.rs` | `motion.js` | motion | The temporal upscaler's motion of the bodies that move on their own, and the first-person view's locked mask |
| Canopies | `canopies.rs` | `canopies.js` | near | The Enhanced tree canopies' leaves |

`core.rs` holds what the passes share: the context and its multi-draw, the
programs, geometries and textures, the inputs the page writes each frame, a
frame block (one uniform buffer) for each view (near, far, first person, a
spare, and the far scene from a water's mirror), and the GL state the
passes leave behind them (`GlState`), which is unknown at the start of
each phase since three draws in between. A phase is one call from the page
that draws its passes in order: `far_frame` (and `far_mirror_frame` and
`far_probe_frame`, the far scene from a water mirror's camera and for a sky
probe's faces), `frame` (the near phase), `transparent_frame`, `view_frame`,
`post_frame`, `motion_frame` and `shadow_frame`. Each returns its stats: the five counts the page has always
read (world draws, instanced draws, instances, program switches, GL calls),
then each pass's draws in the table's order. The near phase is called from
the far scene's image (`graphics.background`'s `onAfterRender`) at the start
of the near scene's pass, and the transparent phase from the item three
draws just before its transparent list (see below); `view_frame`,
`post_frame` and `hud_frame` from their places in `graphics.js` and
`hud.js`, `shadow_frame` from each cascade's pass in `graphics.js`, and
`mirror_frame` (the near phase's passes from a mirror camera)
from a water reflection's render of the near scene.

On the page, `src/rust-render/index.js` loads the module and keeps the
registry. `addPass({ name, phase, register, draw, setActive, restore })`
adds a pass to its phase, and `phase(name, renderer, scene, camera)` writes
the frame block for the phase's view, runs its passes' `draw(ctx)` in order
(each registers what three has drawn once, so its material has a program,
and writes its records), makes the phase's call, then resets three's state
and binds back the target three was drawing into. A pass keeps its own typed
views of the module's memory (`addView`), made again whenever the memory
grows. A pass that must draw at a point inside three's render lists, such as
the start of the transparent list, can put an anchor there (`anchor`): a
mesh three draws with no fragments whose `onAfterRender` calls back.

### What the near scene blends

Blended draws look right only in the order three sorts its transparent
list: group order, render order, the farthest first (the clip-space z of
each object's bounding sphere centre), then the object's id. Whichever
draws the renderer has taken over and whichever three still makes, they
must keep that one order, so the transparent pass owns the list. Every pass
whose draws blend is a source of items for it (`transparent.rs`, `Source`):
each frame it emits an item per thing three would have listed, with three's
sort key and a record of its own, and later draws runs of its consecutive
items. The pass sorts every source's items as three would and merges in
what three still draws itself, whose keys the page reads from three's own
sorted list. The items between two of three's own make a segment: the first
is drawn at the start of three's transparent list, from a hook on the
`onAfterRender` of the item three draws just before it (armed after the
near phase, through the registry's `afterPhase`), and each later one right
after three draws the item before it (`transparent_segment`). The frame
then blends in the order one list sorted by three would have. Each segment
costs three a state reset and its next draw a full upload of its uniforms,
so the fewer of three's own items are left between the renderer's, the
cheaper the list.

The pass's own items are the map's blended buckets and the objects drawn
with three's basic and sprite materials: the command posts' holograms,
their beams and flares. Three still lists them, so its culling, its keys
and the game's own visibility stand: after the near phase, the page takes
those it has registered out of three's list, in the near scene's own render
only (a mirror's still draws them with three), and writes a record for
each: its key, and an object's matrix and material values. The renderer
draws each as three would: a double-sided basic or sprite material twice,
back faces then front, each with three's program for that side, and a
double-sided zero material (a `ShaderMaterial`, which three draws in one
pass) once without culling.

### Shader hooks and the ground

The world pass also draws the map's buckets whose shaders three's hooks
change: scrolling ones (their scroll and the frame block's time), the
banners' wind (`fabric.js`: its `fabricTime` is the frame's time, and a
uniform of a material's that is a frame value under another name is read
from the frame block, its declaration made a `#define` of the block's
member) and the leaves the tree canopies take over (`tree-canopies.js`: the
canopies' state texture and count). The canopies' own mesh is drawn only
while it has leaves to draw (Enhanced tree detail near a canopy).

The ground (`terrain-patches.js`) is one of the world pass's buckets too:
three sorts it among the map's buckets by its material, and its cells are
the patches' own, each a range of the patches' index culled by its box and
drawn in runs, as three's patches draw them, in one multi-draw. Its painted
layers are read from the same texture units in every program. While the
renderer draws the ground, the patches skip their per-render walk over the
cells for the near scene.

A bucket is registered once three has drawn it as its material now is
(three's program for it, and the version it was made for, are current),
not from the program three's warm-up compiled; one whose material changed
since goes back to three until three has drawn it again. A bucket culled by
cell (`render-culling.js`) keeps the draw range of the last view three drew
it in, which the renderer ignores: it draws its cells from the whole index.

### What the transparent list, the hooked buckets and the ground drew

On the emulator (SwiftShader, 800x600, 2x multisampling, Classic
settings), at three points of a seeded walk on each of Theed, Mos Eisley,
Bespin Platforms, Kashyyyk Docks and Hoth, the near scene's image with the
renderer on was compared with three alone on the same frames (time stopped
between the captures), and with the renderer on but each of these passes
given back to three. It was the same to the pixel at every point but one:
on Hoth's third, where two captures of the same variant already differed
by 133 pixels (a distant walker's legs), the renderer and three differed by
103. The
same held with the Enhanced tree and terrain detail, bloom and soft focus
on (the ground's Enhanced shading drawn by the renderer), and with shadows
on, where the renderer gives everything back to three. No WebGL context was
lost during any of these runs.

Three's own draws in the near scene's pass fell from 27 to 15.5 a frame on
Theed, 13.75 to 2 on Mos Eisley, 33 to 3 on Bespin Platforms and 22.5 to
11.25 on Kashyyyk Docks. What three still draws there is the foliage, the
water, the weather and the map's effects, the far scene's image, props'
unlit skinned meshes, and on Bespin Platforms two vehicles' blended skinned
parts. On the emulator with its draws made no-ops, in lockstep frames
alternating between the renderer without these passes and with them, the
frame's render time changed by +0.01 to -0.9 ms (median and mean, two
rounds) on four maps and by -0.7 to -2.6 ms on Mos Eisley; switched every
five seconds in one battle on the game's own loop, the difference was
within the noise.

### The effects

The battle's effects are drawn by the effects pass as items of the near
scene's transparent list (`transparent.rs`'s `Source`): each frame the page
writes each draw with the key three would have sorted it by (group order,
render order, the centre of its bounding sphere through its world matrix,
its id), and the renderer works out z as three's `projectObject` does and
draws the item between the items three still draws, where three would have.
Their simulation stays in the page. Each is registered the first time three
has drawn it, with three's own program, then hidden from three. A particle
batch new this frame (and a soldier, his guns, a rig or a flying part, for
the instance pass) is registered before three lists the scene instead: as
three starts its render of the near scene (the scene's `onBeforeRender`),
three builds its material's program without a draw (`renderer.compile`)
where it has none yet, and the renderer draws it from its first frame:

| What | Page | Kind | Hidden from three by |
| --- | --- | --- | --- |
| Particle batches (Classic bolts and their glow, effects' CPU particles) | `particles.js` | instanced quads | `external` (shown and uploaded nothing) |
| Analytic particles (placed in the vertex shader) | `effects-gpu.js` | instanced quads, emitters texture | `external` |
| Enhanced bolts | `blaster-bolts.js` | instanced capsules | `external` |
| Haze (dust, FogCloud) | `atmosphere.js` | instanced quads | `external` |
| Snow | `snow.js` | instanced quads, data textures | its material |
| Rain | `weather.js` | instanced streaks, base plane | its material |
| Classic arc caster lightning | `lightning.js` | dynamic mesh, two passes | `external` |
| Muzzle flashes, effects' model particles | `game.js` glowModel | models, two passes | its material |
| Droidekas' shields | `droideka-shield.js` | shader model | its material |
| Enhanced rain's ground mist | `weather.js` | the rain's batch, other values | its material |
| Enhanced rain's splashes | `rain-impacts.js` | instanced quads, base plane | `external` |
| Enhanced rain's sheets | `rain-sheets.js` | shader model, never culled | its material |
| Enhanced steam's wisps, wake splashes | `steam-wisps.js`, `fountain-spray.js` | instanced quads, base plane, single-sided | its material |
| Enhanced steam's volumes | `environment-volume.js` | shader model over the screen | its material |
| Enhanced fountains' jets | `fountain-jets.js` | instanced, bases of their own, reading the frame so far | its material |
| Enhanced fountains' soft spray | `fountain-jets.js` | shader models over the screen | its material |
| Enhanced arc caster lightning | `arc-lightning.js` | instanced capsules, kept interleaved | `external` |
| Enhanced droideka's rolling dust | `droideka-trail.js` | instanced quads, base plane | `external` |
| Enhanced jetpack's wake (fire, smoke, embers) | `jetpack-wake.js` | instanced quads, base plane | `external` |
| Enhanced jetpack's flame | `jetpack-volume.js` | instanced boxes ray-marched through a 3D noise texture (unit 21) | `external` |
| Enhanced destruction's fire and smoke | `destruction-fx.js` | instanced quads, base plane, each pool's own attributes | `external` |
| Enhanced destruction's arcs | `destruction-fx.js` | dynamic mesh, as many vertices and indices as its arcs use | `external` |
| Enhanced droideka's aura | `droideka-shield.js` | shader model, front faces, the shield's values | its material |
| Enhanced death camera's trace | `death-trace.js` | dynamic mesh, values of its own | its material |

A batch's instances go into the module's memory attribute by attribute as
the batch keeps them (an analytic batch's from the first that changed since
it was last read; fixed ones once), and the renderer interleaves them into
a vertex buffer of the batch's own with one upload. Three would upload each
attribute of each batch, and every vertex of the lightning's buffers, every
frame. Hiding a dynamic batch from three needs the batch's own flag: three
uploads a visible mesh's changed attributes as it lists it, whether or not
its material is shown. A shader that kept a value of its own under a name the
frame block gives the frame's (the shield's, the rain's, the droideka's
dust's, the jetpack's wake's and flame's, the destruction's, the death
camera's trace's and the shock glow's clocks, their fades) has it renamed,
so its program can take the frame block.

A shader model calls its object's `onBeforeRender` each frame as three
would. The steam's volumes are drawn by three into a target of their own
there (a scene of their own, at half size), and the renderer composites
that image where three would have; the steam's wisps, the wake splashes
and the volumes take the scene's depth whatever the soft particle setting,
so the pass binds it whenever something drawn reads it. The fountains' jets
read the frame drawn so far, blurred through its mipmaps, copied just
before the first jet draws. An object that draws nothing does the copy in
its `onBeforeRender`, ordered just before the jets; three still draws it,
so the renderer's list splits there and the copy is made at the same point
whichever renderer draws the jets. Their two halves of soft spray have a
target each, so that both can be drawn before either is laid over the
frame.

The scene's depth for soft particles is copied where three would copy it.
Three copies it once a frame, in the `onBeforeRender` of the first item of
its transparent list that asks for it, after whatever transparent item
ahead of it writes depth (the Enhanced water's Low and High passes come
first in the list). So as three starts its render of the near scene, before
it sorts it, the pass asks `particle-depth.js` for the copy before each of
its draws whose `onBeforeRender` would take it in three's render (while
soft particles are on, or whatever the setting for those that read the
depth regardless). A copier, an item three draws that draws nothing, is
placed just before the earliest of them in three's order (its centre a hair
farther along the view than that draw's) and makes the copy there; three
draws it, so the renderer's list splits there, as for the fountains' copy
of the frame. The draws' own `onBeforeRender`, which the pass calls as its
phase starts, only learn that the depth will be there. The steam's volumes
and the fountains' soft spray render with the depth at once, in their
`onBeforeRender`: they take a copy of their own as the phase starts, and the
copier still makes its copy for the draws after it. A draw shown only after
three started its render takes the copy as the phase starts. Nothing is
placed without a request, so three alone draws as it did, and a mirror's
render of the scene places nothing.

The flying models (rounds, grenades, droids' pieces, items: `game.js`
flyingModel, tagged `userData.flying`) are drawn by the instance pass as
rigid records in the near phase. Their parts share their materials with
every copy of their model, so a part the renderer draws is hidden from three
by its layers (layer 30) rather than by its material. A part whose material
blends (the scrolling glow of a dropped health power-up) is an item of the
transparent list, as a vehicle's rig that blends is (the jedi fighter's and
the gunship's glass): the instance pass is one of the list's sources, and
draws it with three's program for each side three draws it with, its
blending and depth, at the place three's sort would draw it. A part made
with a heated material (the destruction's burning pieces) takes its heat
and its light from its object, as a dissolving body does. The arc caster's
glow over a struck soldier, his body's buffers and skeleton drawn again in
light, is a rig that blends too, its clock, intensity, seed, inflation and
colour in its row (in the texels a heated body's values use).

Checked on the slow-PC emulator (SwiftShader, 800x600, 2x multisampling,
the Classic looks): at six points of a seeded battle on Bespin, Hoth and
Kashyyyk Docks, walking and firing, the same frame drawn again with the
effects pass on and off (the rest of the Rust renderer on), and with the
whole renderer on and off, differed in no pixel, while the effects drew
hundreds to tens of thousands of them (up to 38 effect draws and 1,956
particles a frame). In alternating lockstep frames of busy battles three's
draws a frame fell from 71 to 62 (Bespin), 50 to 41 (Hoth) and 91 to 74
(Kashyyyk Docks), and Graphics.render's median from 10.1 to 9.8, 9.1 to 8.3
and 15.9 to 14.3 ms.

The same check with the Enhanced environment and rain: on Kamino the rain's
drops, mist, splashes and sheets (about 9,000 instances a frame), with soft
particles off and on, and on Bespin, from two command posts near steam
vents, the steam's wisps and volumes, differed in no pixel at any of the
points. On Theed, walking up to a fountain from two command posts (soft
particles off and on), the fountains' jets, both halves of their soft spray
and the wake splashes the soldier made in the basin differed in no pixel
either.

### The same frame

Splitting the renderer and its page side into passes changed nothing they
draw or call. Against the commit before the split:

- The module alone. Two scenes recorded with `?rustRecord` on Kashyyyk
  Docks with `?rustRigs` (1,082 registration calls and 120 frames of inputs
  each) were replayed under Node into the module from before the split and
  from after it, against a stand-in WebGL2 context that logs every call with
  its arguments and a hash of any data it is given. Apart from the three
  frame blocks the new core makes as it starts, both made the same calls:
  62,152 and 62,171 of them (4,979 draws in the first), with the same data,
  and the same stats in every frame.
- The page side and the module together. A test build carried both, from
  before the split and from after it, made fresh at the same moment in one
  battle on the emulator (SwiftShader, 800x600, 2x multisampling), and each
  drew the same frames in turn at points of a walk. On Theed and Kashyyyk
  Docks, with `?rust` and with `?rust&rustRigs`, the near scene's image and
  the screen were the same to the pixel at every point, and so were the
  stats and the WebGL calls, but for the GPU profiler's timer queries, which
  depend on when the GPU finishes. Whichever side drew first after frames
  three drew alone posed a few soldiers slightly differently than in later
  frames; drawing in a later turn, the new code matched the old call for
  call, the instance rows and bones it uploads included.
- Without `?rust`, builds from before and after the split drew the same
  pixels at the same points of the same seeded battles, but for a few
  hundred pixels of flickering flames on Kashyyyk Docks, which differed as
  much between two runs of the build from before.

### The view, foliage, water, post passes and the HUD

These passes take the rest of a Classic frame's own scenes from three:

- Foliage: each class of placed foliage (grass beds, leaf patches, vines)
  is one instanced draw in the near phase, with three's program for its
  cards. The renderer also chooses the patches each time the page updates
  its foliage (`Foliage.drawer`), as `FoliageGpuBatch.update` does: the
  cells of the patch index around the camera, each patch's sphere against
  the frustum, its distance across the ground and its depth along the view.
  A frame drawn without an update (paused) draws the last selection, as
  three would.
- The first-person view: its parts are rigid meshes with zero materials,
  written as records and drawn instanced as the soldiers' guns are, with
  the view camera's frame block; the muzzle flash (a basic material that
  blends) is drawn after them in three's transparent order, both sides in
  turn as three draws a double-sided one. `RustRender.cover(phase,
  renderer, scene, camera)` does what three's render would do around its
  draws (the scene's matrices and `onBeforeRender`, which packs its lights;
  then resolving a multisampled target), draws the phase, and says whether
  its passes drew the whole scene: `graphics.js` renders the view scene
  with three only when they did not.
- The post passes: `Graphics.postSteps` plans them, each a full-screen
  pass with its target and the uniform values it is drawn with (`values`,
  set on the pass's material by whichever renderer draws the step), then
  hands the plan to the bridge. Bloom is three steps (its extract, then
  its one blur pass twice, across and down, with other values), the
  spatial upscaler one, and the temporal upscaler three (the motion
  resolve, the resolve into its history and the sharpen to the screen);
  the temporal frame's commit (the history swapped, the jitter moved on)
  is the last step's `after`, which `graphics.js` runs once the steps are
  drawn, whoever drew them. The post pass draws every step with three's
  program for the pass's material, used as three compiled it
  (`Kind::Plain`), into a framebuffer of its own over the target's texture
  (the temporal upscaler's history and motion targets are half floats) or
  onto the canvas, or leaves them all to three when one is a step it
  cannot draw.
- The temporal upscaler's motion (`motion-vectors.js`): each body that
  moves on its own (soldiers, the player's guns, vehicles, debris) has a
  proxy sharing its buffers that draws how far its surface moved on screen
  since the last frame, then the first-person view is drawn over that in a
  material that marks it fixed on screen. `MotionVectors.render` keeps its
  per-proxy bookkeeping (last frame's matrices and bones, the first frame
  and teleport rules) and asks the motion pass to draw: each proxy three
  would list (shown, on the camera's layers, in its frustum) is a record in
  three's order (by material: each proxy has its own), drawn instanced with
  three's program for it (`Kind::Motion`: its matrices now and last frame
  and both frames' bone bases read from its row, both frames' bones from two
  textures), then each part of the view scene three's override render would
  list, the view pass's hidden parts included, with the locked material's
  program and the view's camera. The sky's ships, which draw their own
  motion, stay three's and draw first, where the near scene drew nothing (no
  proxy keeps a pixel there). Until three has drawn a proxy of each kind and
  the mask once, three draws the frame's motion.
- The HUD: `BattleHud.render` asks the bridge to draw its batch once it is
  built. The renderer keeps a copy of the batch's vertex arrays, updated
  from the vertices the batch changed, and draws each run of quads with its
  texture, as three does.
- Water: the surface (Low: drawn with the look's own textures; High and
  Ultra: the tint over a copy of the opaque scene, then the reflection),
  the rings and the shore's lapping are items of the transparent list
  (sources of the transparent pass), with three's sort keys (see "The
  Enhanced sky, water and canopies" below).
- Reflections: water at Ultra renders the near scene again from a mirror
  camera, which water.js marks; the near scene's hook then draws the near
  phase's passes for that camera (`mirror_frame`), so what the renderer
  draws shows in the reflection too. The mirror's far scene, at High and
  Ultra, is the far pass's (`far_mirror_frame`).
- The near scene itself, behind `?rustNear` (`near.js`): on a frame when
  everything three would list in it is drawn by a pass, the renderer
  draws the scene in place of three's render. It does what three's render
  does first (the scene's `onBeforeRender`), walks the scene as three's
  `projectObject` does (shown, on the camera's layers, in view) for what
  three would list, and at the first item no pass draws leaves the frame
  to three. Otherwise it clears the target as three would, draws the far
  scene's image with three's program for it, then the near and
  transparent phases (the transparent pass's objects keyed from the walk
  as three keys them), and resolves the target. Phases drawn between
  three's renders (the far scene, the view, the post passes, the HUD, and
  this one) reset three's state once, when three is next used.

If the WebGL context is lost, or a pass throws, the bridge gives
everything back to three for good (`RustRender.fail`): a lost context
takes every object the renderer made with it.

On the slow-PC emulator (SwiftShader, 800x600, the laptop's live settings
with 2x multisampling), each pass switched on alone against three in the
same frames of one battle drew the same pixels: Theed's foliage, FXAA,
HUD and first-person view (also while firing), Kashyyyk Docks' foliage and
water, and Theed's reflection at Ultra (0 of 1,440,000 pixels each over
three points, 0 of 960,000 for the reflection over two). With draws off
and the emulator's CPUs throttled, frames alternating between three and
the pass (median and mean, ms): the foliage's update on Naboo: Plains,
whose terrain grows 197,000 patches, went from 3.46 and 3.90 to 0.50 and
0.57 and the frame from 27.7 and 29.0 to 22.2 and 23.6; on Kashyyyk Docks
from 0.135 and 0.165 to 0.055 and 0.067. The HUD's render went from 0.90
and 1.19 to 0.77 and 1.01, and three's render of the first-person view
(0.56 a frame) became 0.16 of three's plus 0.17 of the renderer's. The
post passes and the water were within noise: each phase costs about as
much as a scene of one or two draws saves (the page's side, the call, and
three's state reset after it, which three then sends again), which only
a frame drawn with fewer phases will save.

### The far scene

`far.rs` and `far.js` draw the far scene (the sky dome and the models on
its horizon, the map's low-detail buckets, the far terrain, the far water
and the sky's flyers) in place of three's render of it. graphics.js asks
the core to cover the far phase (`cover("far", ...)`): three's start of a
render (the far scene's world matrices, its onBeforeRender), the phase,
and, when the far pass drew all of the scene, three's end of one (the
multisampled target resolved). Three renders the far scene only when the
pass could not draw all of it.

Each frame the pass walks the far scene as three's render does (what is
shown, on the camera's layers) and finds each mesh three would draw among
its items, registering one the first time its material has a program. An
item is a program made from the GLSL three compiled for its material, its
geometry and state, and where each uniform the program reads comes from: a
fixed value, a value the page writes each frame, a texture, the camera, or
the object's own matrices (computed as three computes them, in doubles).
A material that reads only the frame's shared values takes them from the
far camera's frame block; any other, such as the water's, which keeps a
time of its own, gets a program of the Plain kind, three's GLSL as it is,
with every uniform set by the pass. The map's buckets are culled by spatial
cell as render-culling.js culls them, each visible run in one multi-draw;
the terrain's cells by the frustum and the fade start as terrain-patches.js
culls them, its visible runs in one multi-draw too; the flyers draw
instanced from the matrices the page writes each frame. A sampler a
material gives no texture reads a texture of one texel of zeros, as three's
empty texture is (Hoth's hangar platform in the far scene has no map). The
items are sorted as three sorts its render lists and drawn with a uniform
or a piece of state set only when it differs from what is set.

In one battle with `?rust`, at four points of a walk, the far pass and
three's far render drew the same lockstep frame in turn (two settle frames,
then a captured one) on the emulator (SwiftShader, 1599x899, 4x
multisampling). On Naboo: Theed, Tatooine: Dune Sea, Bespin: Platforms,
Hoth: Echo Base, Kashyyyk: Docks and Geonosis: Spire the far image and the
near scene's image over it were the same to the pixel at every point (5.75
million pixels a map), and so were two frames three drew alone; aimed at
the flyers on Tatooine: Dune Sea and Geonosis: Spire (a ship on screen, up
to 108,000 pixels of it), the two were the same to the pixel too. In each
of the far pass's frames three drew none of the far scene:

| Map | Three's far draws a frame | Far pass's draws (WebGL calls) | Far scene's part of the frame, ms (p50) | Graphics.render, ms (p50) |
| --- | --- | --- | --- | --- |
| nab2 | 46 | 37 (152-155) | 1.12, 1.50 -> 0.61, 0.81 | 5.86, 8.25 -> 5.30, 7.30 |
| tat1 | 22.5 | 13-16 (106-112) | 1.28, 1.13 -> 0.85, 0.81 | 8.06, 7.59 -> 7.50, 6.71 |
| kas2 | 26 | 17-21 (119-122) | 1.23, 0.99 -> 0.79, 0.70 | 9.23, 8.80 -> 8.48, 8.28 |
| bes1 | 14 | 14 (88) | 0.91, 0.65 -> 0.63, 0.48 | 8.47, 5.92 -> 8.43, 5.93 |
| hot1 | 12.6 | 6 (61) | 0.70, 0.62 -> 0.62, 0.59 | 6.94, 6.52 -> 6.60, 6.64 |

The times are the slow-PC emulator's (draws switched off, CPU throttled as
the laptop runs), from lockstep frames alternating between the two in one
battle, two runs a map: three's render of the far scene against
`cover("far")` with all it does, the far scene's matrices and hooks and the
target's resolve included. About half of what is left is the renderer's
call and its WebGL calls (some 0.3 ms on Theed); the rest is the page's
side of the pass, the far scene's own hooks (the terrain's runs for three,
the tree canopies), three's state reset after the phase and the resolve.
The map's far buckets whose programs read the same attributes laid out
alike share one vertex array (their vertices in one buffer, their indices
in one, each bucket drawn from where its own index starts), which took
Theed's far pass from 185 WebGL calls a frame to 154.

With Enhanced terrain, trees, ocean and sky, water at High, the full far
terrain, the far view distance and 4x multisampling, the far scenes of
Theed and Hoth were the renderer's alone and the same to the pixel. The
Enhanced sky, water at High and Ultra and the sky probes, which render the
far scene again from cameras of their own, are the far pass's too: see the
next section, as is the Enhanced ocean's. Shadows, lamp lighting and the
temporal upscaler need nothing of the far pass itself (its materials read neither shadows nor lamps, and
the upscaler's jitter is in the far camera's projection), only the core's
leave to stay on with them.

### The Enhanced sky, water, canopies and ocean

- A water mirror's far scene. Water at High and Ultra renders the far scene
  again each frame (every other frame at High while the eye holds still),
  from a camera mirrored in the surface, into the reflection's target: the
  sky's models through the plain projection, then the rest over them
  through the oblique one, without clearing. Each render is offered to the
  far pass first, as a phase of its own (`farMirror`) with a frame block of
  its own, so that the eye's far scene, drawn later in the frame, reads its
  own. The far input says whether three would clear (its autoClear).
- The sky probe. The Enhanced sky's flyers take on the sky behind them far
  off, and the Enhanced ocean under a dry sky reflects it: both read a cube
  map of the far scene's sky around the eye (sky-probe.js), captured once a
  second or on a 4 m climb. A cube sampler's source names the cube each
  frame and binds it by its target. The capture is the far pass too: the
  six faces drawn as `CubeCamera.update` draws them, each offered to the far
  pass as a phase of its own (`probe`) with a frame block of the pass's own
  for each face; after the last face the pass makes the cube's mipmaps as
  three's render does.
- Water at High and Ultra. The near scene's two passes over the surface
  (the tint over a copy of the opaque scene's colour and depth, then the
  reflection, which reads the mirror's image and, at High, marches through
  the copy) read the wading soldiers, an array of eight `vec4`: a water
  program may read one array, set when it differs from what the program
  holds. The tint pass copies the opaque scene in its `onBeforeRender`, just
  before it draws; it is the first item of the transparent list (the lowest
  render order there), so the pass makes the copy at the start of the
  transparent phase. The shore's lapping and splashes (water-shore.js) draw
  one quad instanced at each shore point from instance buffers of the
  pass's own, filled again whenever three would upload them.
- The tree canopies. With Enhanced tree detail the jungle backdrop's models
  near the eye are rebuilt as canopies of camera-facing leaf cards
  (tree-canopies.js), one instanced mesh whose leaves are written slot by
  slot as the groups near each camera change. Once three has drawn the mesh,
  the canopies pass draws it in the near phase (a mirror's too) with three's
  program for it, and three neither lists the mesh nor uploads its leaves:
  each frame the page copies the instances whose slots changed into the
  renderer's copy. They are drawn after the near scene's world, soldiers,
  ground and foliage, as three's own draw of them already came after the
  renderer's world. This order differs from three alone on purpose, as the
  far scene's image drawn last does: three sorts the canopies by their
  material's id, which on Naboo: Theed puts them before every one of the
  map's buckets, so every leaf fragment is shaded and most are then
  covered; drawn after what hides them, the hidden ones fail the depth test
  before they are shaded (on the slow laptop the frame's GPU time there fell
  from 33 to 20 ms, below). Where a leaf card and a surface meet at the same
  depth, the order decides the sample: at three points of a walk on Theed,
  three alone with only the canopies moved after every other opaque item
  gave the renderer's image to the pixel at all three, and differed from
  three as it is by 0, 8 and 0 pixels.
- The Enhanced ocean. The sea's surface is a mesh of its own in the near and
  far scenes, with crest spray over it, and on a sea the map rains on
  (Kamino) a storm: a panorama of clouds drawn over the sky by a
  full-screen triangle, a horizon veil, the horizon's models in the storm's
  light, and lightning drawn from vertices rewritten at each strike. The
  far pass draws a mesh with no index through an index of its own (0, 1,
  2...) over the range three draws each frame, and refreshes a mesh's vertex
  buffers when three would upload them, so the storm is far items like any
  other. The near surface shares its material with the far one, so it is the
  far pass's item too, drawn from the near phase after its opaque passes and
  hidden from three on a layer no camera renders. The spray is an instanced
  surface of the water pass.

On the emulator (SwiftShader, 1600x900, four samples, the Classic looks
with the feature on), at four points of a seeded walk each, the presented
frame and the targets the features draw into were compared between three
alone, the renderer, three alone again, and the renderer with that feature
given back to three, with the eye put 5 m over a water cell (water), aimed
at the nearest flying ship (sky) or canopy (canopies), the reflection drawn
afresh and the probes captured afresh in every compared frame:

| Feature | Maps | Compared | Pixels that differ |
| --- | --- | --- | --- |
| Water at High (far mirror, surfaces, shore) | Kashyyyk: Docks, Naboo: Theed | frame, reflection | 0 at every point |
| Water at Ultra (far and near mirrors, surfaces) | Kashyyyk: Docks, Naboo: Theed | frame, reflection, its far image | frame 0; reflection 1 and 247 at one point each, as many as three against itself there |
| Enhanced sky (flyers, probe) | Tatooine: Dune Sea, Bespin: Platforms, Geonosis: Spire | frame, far image, the probe's six faces | 0 at every point |
| Enhanced trees (canopies) | Yavin 4: Temple, Naboo: Theed (and at Ultra) | frame | 0 on Yavin 4; on Theed 1 and 8 pixels at one point each, as many as three alone gives with only the canopies drawn after the rest (0 against the renderer) |
| Enhanced ocean | Kashyyyk: Islands, Kamino (its storm) | frame, far image, the probe's six faces (Islands), the storm's clouds (Kamino) | 0 at every point |

In the renderer's frames three drew none of the far scene, the mirror's far
scene or the probe's faces; on Kashyyyk: Docks at High, three's draws a frame
fell from 88 to 1, the far mirror drawing 21 and the water 10.

With the High quality preset (shadows, lamps, the temporal upscaler with
its jitter pinned and history reset for each compared frame, the shadow
caches drawn again before each), the same comparisons gave 0 pixels at every
point for water at High (Kashyyyk: Docks), the Enhanced sky (Bespin:
Platforms), the ocean (Kashyyyk: Islands, Kamino) and the canopies (Yavin 4:
Temple); with water at Ultra the frame and the mirror's far image were the
same to the pixel, and the mirror's near image differed at two points by as
much as three's own two renders of it did. With the Balanced preset (Low
water, soft particles) Kashyyyk: Docks and Naboo: Theed were the same to the
pixel. In those frames three drew none of the far scene, the mirrors' far
scenes, the probe or the water.

On the emulator with its draws made no-ops, a water mirror's far scene on
Kashyyyk: Docks at High, drawn on the same frames alternately by the far
pass and by three, cost 0.73 ms (median) and 1.01 ms (mean) with the far
pass against 1.05 and 1.50 ms with three's two renders. In lockstep frames
of one battle alternating, in blocks, between the renderer with these
features and the renderer with them given back to three (as before them),
the eye at the feature, Graphics.render took (median and mean, ms):

| Map, feature | Given back to three | Drawn by the renderer | Three's draws a frame |
| --- | --- | --- | --- |
| Kashyyyk: Docks, water at High | 3.94, 4.30 | 3.85, 4.12 | 7 -> 2 |
| Bespin: Platforms, Enhanced sky | 5.11, 5.63 | 4.88, 5.55 | 20 -> 2 |
| Kashyyyk: Islands, Enhanced ocean | 3.94, 4.25 | 3.39, 3.68 | 13 -> 2 |
| Kamino, Enhanced ocean and storm | 2.76, 3.24 | 2.44, 2.70 | 9 -> 2 |
| Naboo: Theed, Enhanced trees | 6.29, 6.62 | 6.37, 6.71 (within the noise) | 3 -> 2 |

The water's reflection and the sky probes are drawn before Graphics.render
and are not in these times (the reflection's is above).

On the slow laptop (fast FXAA, no multisampling, full resolution, each
feature's setting on, the eye at the feature), three alone against the whole
renderer, lockstep blocks in one battle, three rounds (means):

| Map, feature | Graphics.render, ms | GPU process CPU, ms a frame | GPU time of Graphics.render, ms | GPU time before it (mirrors, probes, clouds), ms |
| --- | --- | --- | --- | --- |
| Kashyyyk: Docks, water at High | 9.67 -> 6.15 | 12.3 -> 9.1 | 134.0 -> 137.0 (rounds 128 to 147) | 0.58 -> 0.47 |
| Kashyyyk: Docks, water at Ultra | 5.86 -> 6.04 | 25.8 -> 21.7 | 48.3 -> 47.9 | 10.5 -> 7.5 |
| Bespin: Platforms, Enhanced sky | 9.69 -> 6.88 | 18.7 -> 15.9 | 15.4 -> 14.8 | 0.01 -> 0.01 |
| Naboo: Theed, Enhanced trees | 8.38 -> 5.65 | 20.9 -> 17.5 | 33.1 -> 20.3 | 0 -> 0 |
| Kashyyyk: Islands, Enhanced ocean | 6.76 -> 6.73 | 13.6 -> 10.3 | 81.0 -> 81.3 | 0.01 -> 0.02 |
| Kamino, Enhanced ocean and storm | 9.25 -> 4.62 | 9.0 -> 6.2 | 86.3 -> 85.5 | 2.19 -> 1.94 |

The first block on Kashyyyk: Docks at High (three's, 17.9 ms, just after the
load) weighs on its mean; its other rounds were 5.2 to 5.9 ms with three and
5.8 to 6.7 ms with the renderer. The GPU time on Naboo: Theed falls with the
canopies' order (above). In these views the laptop is held back by its GPU
with water at High (its reflection's march over the screen) and with the
Enhanced ocean: their shaders, the same whichever renderer draws them. The probe's capture,
six faces once a second, costs a little more drawn by the renderer: each
face is a phase of its own, with three's state reset after it, and the sky
flyers' update (which captures it) took 0.11 ms a frame on average against
0.08 ms with three.

### Shadows, lamps and the temporal upscaler

The renderer covers the configuration a battle starts in, the Enhanced ones
included (the Balanced and High quality presets have shadows, lamp lighting
and the temporal upscaler on). A change of any of the three in mid-battle
gives everything back to three for the rest of it, as the programs made for
the old configuration would be stale. Within a configuration, a program
that reads what the renderer can't give it is three's to draw, item by item:
a draw stays three's, not the frame.

- Shadow receivers. With shadows on, three compiles every zero material of
  the near scene with its shadow uniforms: each cascade's matrix, the
  matrix that carries the receiver's plane into it, its slice of the view
  and its shadow values, and the depth maps. The renderer's programs read
  them from one uniform block, BfeShadow, written once a frame from the
  values three would upload to each program; the depth maps are bound on
  units of their own. shader.rs makes the block only of a program three
  compiled for shadows, so a Classic program's source is as it was. A world
  draw's receiveShadow is its mesh's.
- Lamp lighting. Every lamp source of the map is in one texture, two texels
  a source (its position and reach, its colour). A DYNAMIC draw reads its
  four lamps from it by their indices (an instanced or first-person draw
  from its row, a world draw from uniforms of its own), chosen by
  local-lights.js as three's onBeforeRender would choose them, with the same
  per-object cache and occlusion rays.
- The shadow cascades. graphics.js offers each cascade's pass to the
  renderer's shadow pass (the static cache the frame its window lands, the
  live map each frame it is drawn), and three draws one only when the
  renderer can't draw all of it. The scene is walked as three's shadow pass
  walks it, once a frame for each set of layers (three walks it for each
  cascade): what is shown, on the pass camera's layers, casting, in the
  cascade's frustum, each with its depth material dressed as three's
  getDepthMaterial dresses it and drawn as three compiled it, over the range
  three would draw. The renderer draws into a framebuffer of its own over
  the map's depth texture, cleared, or given the cache's depth as
  graphics.js copies it. A caster whose geometry the page rewrites (the
  crowd's shadow mesh, its index packed again as soldiers come and go) is
  sent again as three sends it, by its update ranges, and an index only as
  far as the pass draws it: the crowd's mesh draws a tenth of its index.
- Three's own renders that must still see what the renderer took from it
  (a cascade three draws itself, the motion vectors while three draws them)
  run with it shown (`reveal`): each pass shows three what it hid, then
  hides it again. The shadow pass walks the scene inside it too.
- The temporal upscaler's jitter needs nothing: the frame block is written
  after the upscaler has moved the camera's projection, and the page and the
  renderer cull with the same matrix three does.

Counted draw by draw on the five maps below with the Balanced and High
quality presets, three draws 4 to 7 of a frame's 105 to 250 draws (2 to 6%):
the far scene's image, an anchor the renderer draws nothing with, a skinned
mesh and one other mesh of the near scene, and, where the sky's ships fly
(Dune Sea; Bespin Platforms at High quality), their motion, which they draw
themselves. Three drew no shadow cascade.

On the slow-PC emulator (SwiftShader, 1600x900, four samples where the
settings antialias), at four points of a seeded battle on each of Theed,
Dune Sea, Bespin Platforms, Hoth and Kashyyyk Docks, the frame drawn with the
renderer was the same to the pixel as three's with the Balanced preset (its
jitter fixed and its history reset for each capture), with shadows alone at
two and at four cascades (the static caches drawn afresh at each capture)
and with the Classic settings. Before the lamp texture upload fix, with lamps alone it was the same on four of
the maps; on Bespin Platforms, at one point of the four, 1,513 to 1,774
pixels (0.11 to 0.12%) differed in five runs, by one level for the most
part, all on the lamp-lit side of the soldier nearest the eye, with or
without multisampling and bloom; with lamps off there, none did.

With the emulator's draws switched off and its CPU throttled as the laptop
runs, in one battle, lockstep frames alternating between three alone and the
renderer (240 each), the median of Graphics.render went (ms):

| Map | Balanced | High quality | Shadow pass, Balanced | Shadow pass, High quality |
| --- | --- | --- | --- | --- |
| Theed | 23.8 -> 19.6 | 106.5 -> 85.7 | 4.3 -> 3.4 | 6.1 -> 3.7 |
| Dune Sea | 20.2 -> 17.5 | 67.2 -> 49.9 | 4.9 -> 3.8 | 7.7 -> 4.8 |
| Bespin Platforms | 18.7 -> 17.5 | 128.0 -> 105.9 | 4.2 -> 3.5 | 7.2 -> 4.3 |
| Hoth | 18.9 -> 17.6 | 110.4 -> 102.9 | 5.8 -> 4.4 | 10.8 -> 6.8 |
| Kashyyyk Docks | 28.2 -> 23.9 | 80.2 -> 63.0 | 6.2 -> 4.6 | 7.9 -> 4.7 |

Three's own draw calls a frame (median) fell from 93-234 to 26-69 with the
Balanced preset and from 129-264 to 27-77 at High quality. Of what three
still draws at High quality, the skinned meshes with the DYNAMIC material
(the soldiers' bodies among them) cost the most: on Theed their 43 draws a
frame took 21.8 ms of three's near render, the 15 DYNAMIC meshes without a
skeleton 8.7 ms, nothing else 1 ms. Three sends every uniform of a
receiver's material again each time the same material draws again
(materials.js drawsAgain, for each draw's point lights and lamps), the
shadow arrays included: with three alone, one of those skinned draws cost
0.57 ms with four cascades against 0.095 ms without shadows.

## His controls on a slow CPU

His own controls run on the input clock (`src/runtime/reconcile.js`): a
command for each 60 Hz tick, stamped for the world tick it should run on
plus a margin (`InputLead`), his body predicted by replaying the commands
the world has not acknowledged, and drawn at the clock's phase. Recorded
frame by frame on a slow-PC emulator (four throttled cores calibrated
against the A8-6410 laptop; Theed, medium AI, on foot, 26 to 30 FPS),
what made them stop or lag:

- **The lead pinned at its cap after the page stalled.** When the page
  stops for a third of a second or more, the world worker's packets queue
  up, and the browser draws the next frame before it has taken them all.
  That frame stamped his commands from a newest tick some way behind the
  world, the 12-tick cap held them there, and they reached the world 5 to
  100 ticks late. Each late acknowledgement raised the extra margin by its
  lateness: a 0.3 s stall raised it to 7 ticks, one of 0.6 s or more to
  the cap, 12 ticks (200 ms), and offline it comes down a tick per ten
  seconds. For about two minutes after, each correction replayed 14 ticks
  instead of 6 or 7 (about 1 ms more of a frame while he walks), and the
  input clock ran within a few ticks of its bound (the target plus 7): a
  world stall of 160 ms stopped his controls for 60 to 80 ms, one of
  240 ms for 90 to 150 ms. With the lead in place a 240 ms world stall did
  not stop them.
- **The first second and a half of a battle, and of every resume.**
  Packets that came just before the worker stopped (behind the loading
  screen, the pause menu) kept their arrival times from before, which put
  the lead's estimate of the world seconds ahead and the lead at its cap
  until some 60 newer packets had pushed them out.
- **The page's own lateness read as the world's.** The lead took a
  packet's arrival as when the page got round to it: on the emulator 16 ms
  after the worker posted it at the median, 34 ms at the 90th percentile.
  It read the differences as the world bunching its ticks (its arrival-gap
  term at 0.8 of a tick, where the worker's own post times put it at
  0.25), and stamped his commands a tick further ahead than they needed.
- **A world that cannot keep real time.** In a busier battle (Hoth, 28 a
  side) the emulated world ran at 87 to 94% of real time, its step 14 to
  16 ms at the median and 21 to 24 ms at the 90th percentile. Its worker
  gives up the ticks it cannot run but reported none to the page, so the
  lead's estimate ran ahead of it and the lead sat at the cap much of the
  time, the input clock at its bound 19 ticks above the newest packet:
  it stopped whenever the packets paused (0 to 7 stops of 50 to 100 ms
  in 90 s).

With the lead where it belongs the input clock never stopped in the
emulator's ordinary Theed recordings, where the world's longest gap
between two ticks was 46 to 78 ms.

### Changes

- Lateness on a command stamped while the cap held the target
  (`InputLead.capped`, the `capped` argument of `acknowledge`) neither
  raises the margin nor resets its calm count: no margin moves a stamp the
  cap holds. The cap, the margin, the bound and the recovery rate are
  unchanged. Online the cap sits 24 ticks above the extrapolated world
  clock and sets stamps only past 300 to 375 ms of round trip.
- Offline, a packet that came before the worker's clock went on again
  counts, for the lead, as arriving when it went on (`resumedAt` in
  `src/game.js`).
- The worker stamps each post with when it left, on the clock the page and
  the worker share (`local-message-batch.js`; the page never took a post
  before its stamp), and the lead places the world's tick by that; the
  playout clock keeps the page's own arrival times, which are when it can
  draw the packets. Online the transports pass no send time. The estimate
  then also holds through a page stall long enough to fill its window
  with packets taken late (on the emulator, where the page gets a packet a
  frame, even a 4.5 s stall did not fill it).
- His replay takes keys and look only from the commands the world gets,
  and look as the world takes it. The page sends the world a frame's last
  command (and the first when held keys change), each carrying the running
  total of his mouse look, and the world takes the change in that total
  since the last command it took as one sample on that command's tick
  (`InputMailbox.take`). The replay turned him by each command's own
  samples on the frame's first tick, so while he turned, the world's state
  for a corrected tick was the predicted one in only 20 to 32% of
  corrections and his drawn body was pulled about a thousand times a
  minute. The sender marks each command with whether it went, the look the
  world takes with it and whether its own look has gone
  (`network-inputs.js`); look not gone yet is shown over the aim. Hulls,
  mounted guns and the recon droid replay through the same merge: a hull
  steered by mouse had matched its prediction too seldom to go on.

### Measurements

The emulator, Theed, medium AI, on foot; 40 s recordings, each build run
twice interleaved with the build before it. Ranges are the runs'.

| His controls | Before | After |
| --- | ---: | ---: |
| Margin after a page stall of 0.3, 0.6, 1.2 and 2 s | 7, 12, 12, 12 ticks | 0 |
| Lead after one long stall, p50 (p90) | 12 (12) | 4.0 to 4.2 (7.6) |
| Ticks pending at a correction, p50 | 14 | 6 to 7 |
| Page simulation phase, p50 | 7.1 ms | 6.0 to 6.7 ms |
| Frames at the cap in a battle's first 6 s (a run each) | 36 of 241 | 1 of 242 |
| Lead with packets timed by the worker, p50 (p90) | 4.0 to 4.4 (6.2 to 6.5) | 3.2 to 3.8 (5.5 to 7.4) |
| Corrections the world's state matched, walking and turning | 19.6 to 31.8% | 99.3 to 99.8% |
| Corrections that moved his drawn body, a minute | 1,042 to 1,057 | 1 to 28 |
| Frames his drawn body was off its prediction by over 1 mm | 40 to 48% | 1 to 3% |
| Hull corrections matched, driving and steering by mouse (a run each) | 55 of 506 | 472 of 472 |

"One long stall" is the measuring harness's start, which began the battle
before the loading screen's last frames, so the page's first draw of it
(some 4 s linking shaders under SwiftShader, which has no parallel shader
compile) came after the battle had begun. In the loading screen's own order
that draw comes before, and no command was late at the start. With the
lead reset by hand the first two builds measured the same (lead 4.0 to
4.7, 7 ticks pending).

World stalls (the worker held in a loop, three of each length) stopped
the input clock, with the lead pinned, for 60 to 80 ms at 160 ms and 90
to 150 ms at 240 ms; after these changes none of 120 or 160 ms did, one
of three of 240 ms did (for 70 ms), and those of 320 ms for 90 to 180 ms.

On Hoth (90 s from the battle's eighth second, two runs before and three
after) the runs spread more than the builds differed: 2 and 4 stops of
50 to 83 ms before, 7, 0 and 1 of 50 to 100 ms after; the lead at the
median 12 and 6.4 ticks before, 7.5, 9.1 and 5.0 after, its 90th
percentile at the cap in every run.

The input clock's jumps past wall time (60 to 100 a minute, a tick or so
a second in all) and its 10% catch-downs measured the same with each
change, as did key-to-body latency: W pressed at a random moment moved his
drawn body 33 to 40 ms later at the median, 45 to 50 ms at the 90th
percentile (one or two frames; 5 to 7 presses a run where nothing blocked
him), pinned lead or not. His own movement is predicted; the lead delays
only what the world decides.

### What is left

- The page's own long frames still freeze everything while they last; the
  changes only stop them from leaving his controls worse afterwards.
- Every new correction replays every pending tick: 5 to 7 at the lead's
  usual size, 0.7 to 1 ms of a frame (2.5 ms at the 90th percentile). A
  hull's prediction goes on when the world's state for the corrected tick
  is the one predicted for it, and a soldier's now nearly always is. His
  could go on too, but a replay of his starts from more than his body: his
  recoil and every weapon's state would have to match as well, and the
  crowd push takes other soldiers where the page has them now, which a
  prediction going on would not see again. The few corrections left differ
  in his position first.
- A world that cannot run at real time still holds the lead at its cap
  and the input clock at its bound. Reporting the ticks the local world
  gives up (its acknowledgements always say none, so the estimate's reset
  for them never fires) was tried on Hoth: the lead's 90th percentile
  came off the cap (8.2 and 9.9 ticks against 12), but each report lowers
  the target, and the bound with it, under a clock already ahead of it,
  and the clock stood still more and longer (9 and 10 stops in two 90 s
  runs, up to 317 ms). The input clock's 90% catch-down sheds at most six
  ticks a second, fewer than a world at 87% of real time gives up: a
  world step that keeps up is what helps.
- Offline the margin still comes down a tick per ten seconds after
  lateness the world itself causes (a tick that ran long holding his
  commands in its batch); recovering it as online does is a tolerance
  change, left for a decision.

## The soldiers' movement in WebAssembly

A soldier's movement tick (`Soldier.tick`, src/soldier.js) runs for every
soldier in the world worker's step, and the page runs it again for his own
soldier every frame, over each command the world has not acknowledged yet
(the replay above). In the emulated worker's profiles it was a tenth of the
step on Naboo Plains and a twentieth on Dune Sea and Bespin Platforms, and on
the page about half of what the replay cost.

`rust/sim/src/movement` runs the whole tick in the ray kernel's module, for a
battle whose query has `movement=wasm` (which loads the ray kernel with it):
the world worker's ticks (and a dedicated server's) and the page's replay
alike, each with its own instance of the module. The offline world worker
uses it unless the battle's query has `movement=js` or `rays=js`; a page and
a dedicated server keep the JavaScript unless asked.

### What it computes, and how it is called

The module repeats soldier.js's tick: the posture, jump, dive and fall states,
both kinds of jetpack, the drive along the ground, and the collision pass (the
terrain's column under each body sphere, the nearest polygon of the buildings
and bodies one contact at a time, the step up stairs, the ground snap and the
headroom ray). Its queries are the module's own (the sphere gathers, the
overlap test and the ground of `rust/sim/src/collision/spheres.rs`, and the
casts of `collision/mod.rs`), against the copies of the trees, the terrain
grid and the bodies
the ray kernel keeps anyway, so a tick makes no call back into JavaScript.

The soldier object stays the only copy of his state between ticks, so the
bots, seats, corrections and explosions that read and set its fields go on as
before. `src/movement-kernel.js` copies about forty numbers into the module's
slots (his state, his input and the class's per-tick numbers), makes the call,
and copies 27 back. Bodies are switched on and off by a plain field, so each
one's `enabled` goes in before every tick, in the world's order (59 to 120 of
them on these maps). A soldier gets an id in the module on his first tick
there, with his class's numbers, and the module keeps his body spheres' gather
caches. A tick in a state the module does not know (an air or posture of
another kind) is left to the JavaScript.

### The same answers

Each method repeats the JavaScript's operation by operation, in the same
order, in f64, with three.js's Vector3 and Triangle methods spelt out as they
compute; Math.min and Math.max keep JavaScript's rules for NaN and signed
zeros. Math.hypot of two numbers is V8's own algorithm (the larger magnitude
divides both, the squares are summed with Kahan's compensation, and the root
is scaled back), which matched V8 on ten million pairs in both Node 22 and
Electron 44. The sines and cosines a tick reads (of the aim's yaw before
and after its turn and, for a hovering jetpack, of its pitch) are computed in
the module once each, as the production server's V8 computes Math.sin and
Math.cos (`rust/v8math`, V8 13.6's fdlibm): the same as the JavaScript tick's
in Node, and the server's in a game client whose own V8 (Electron 44's)
parts from it in the last bit of some of them, so a client's soldiers move as
the server's do.

- `tests/movement-kernel.test.js` ticks soldiers and their twins in one
  world, one by the JavaScript and one by the kernels, and compares every
  field the tick reads or writes with `Object.is`: eighteen bots of six
  classes on a course (stairs, walls, ramps, a low slab, a tower, terrain,
  props switching on and off, two moving hulls) for 1,500 ticks, then
  scripted climbs, a crawl, a fall and dives.
- `tools/world-hash.mjs --movement wasm` gave the same fingerprint as the
  default for every tick: 2,400 ticks on 24 maps, sides, seeds and modes
  (Naboo Plains, Dune Sea, Bespin Platforms and Hoth in several of each, seven
  other maps, acknowledgements, a page's interval and shots judged rewound),
  and 6,000 to 9,000 ticks on seven more.
- `tools/prediction-timing.mjs`, `tools/own-player-correction-check.mjs` and
  `tools/ground-player-correction-check.mjs` with `BFE_MOVEMENT=wasm` print
  the same results and the same digests of every replayed tick as without it.

### Measurements

On the emulated laptop and on the laptop itself, one battle at a time
(medium AI, on foot, walking and strafing), the kernel was switched on and off
every 5 seconds on the page and in the world worker together, 16 periods in
ABBA order, with each soldier tick and each world step timed: two battles a
map on the emulator, two on the laptop for Naboo Plains and Bespin Platforms
and one for Dune Sea and Hoth. Both variants have the ray kernel, whose
gathers already answer the JavaScript tick's queries. Means of the periods
(the median and 90th percentile, of each battle's), JavaScript then
WebAssembly; gaps are counted over the battles:

| | Naboo Plains | Dune Sea | Bespin Platforms | Hoth |
| --- | ---: | ---: | ---: | ---: |
| Emulator: soldiers' ticks, ms a world tick | 0.87, 0.59 | 1.16, 0.80 | 1.16, 0.74 | 1.24, 0.87 |
| World step median, ms | 8.07, 7.90 | 17.71, 16.61 | 16.18, 15.94 | 14.75, 14.78 |
| World step 90th percentile, ms | 13.56, 14.06 | 25.56, 24.78 | 23.30, 23.01 | 21.12, 21.15 |
| Ticks a second | 67.2, 67.1 | 58.6, 60.8 | 63.1, 63.1 | 65.1, 65.4 |
| Gaps between ticks over 40 ms | 7, 7 | 24, 16 | 21, 17 | 19, 10 |
| His replay's soldier ticks, ms a frame | 0.35, 0.18 | 0.53, 0.26 | 0.53, 0.36 | 0.32, 0.18 |
| The page's simulation phase median, ms | 5.58, 5.42 | 7.21, 7.48 | 6.74, 6.60 | 6.37, 6.18 |
| Laptop: soldiers' ticks, ms a world tick | 1.20, 0.81 | 1.28, 0.86 | 1.02, 0.68 | 1.40, 0.98 |
| World step median, ms | 10.75, 10.60 | 18.95, 17.23 | 17.99, 17.85 | 15.51, 14.90 |
| World step 90th percentile, ms | 14.90, 15.17 | 24.98, 22.94 | 22.44, 21.81 | 19.59, 19.43 |
| Ticks a second | 70.7, 70.0 | 58.1, 63.6 | 62.0, 62.6 | 69.2, 69.5 |
| Gaps between ticks over 40 ms | 0, 4 | 16, 9 | 22, 18 | 6, 2 |
| His replay's soldier ticks, ms a frame | 0.30, 0.14 | 0.56, 0.29 | 0.78, 0.50 | 0.39, 0.25 |
| The page's simulation phase median, ms | 6.92, 6.78 | 8.89, 8.68 | 8.36, 7.97 | 7.21, 7.18 |

The soldiers' ticks take about a third less, 0.28 to 0.43 ms a world tick,
and steadily: in all 112 pairs of neighbouring periods the kernel's were the
shorter. The world step falls by about as much, which is less than it
wanders from period to period as a battle goes on (0.5 to 2.4 ms between
neighbours), so one map's change in the step is mostly within its noise; over
all the pairs the kernel's step was the shorter in 74 of 112. Dune Sea's
world, the slowest, kept more ticks a second with the kernel on both
machines, and the gaps over 40 ms were fewer on six of the eight map and
machine pairs. On the page his replay's soldier ticks take 35 to 55% less,
0.13 to 0.28 ms a frame (4 to 14 replayed ticks a frame), within a simulation
phase of 5 to 9 ms that wanders by more than that.

Under Node on the development machine, switching the kernel every tick of a
3,000-tick battle, a soldier's tick took 10.6 to 21.2 µs with the kernel
against 16.3 to 37.1 µs without it, and the soldiers' ticks 0.48 against
0.81 ms a world tick on Naboo Plains, 0.60 against 0.81 on Dune Sea, 0.67
against 1.17 on Bespin Platforms and 0.45 against 0.69 on Hoth.

### What is left

- What a tick costs around the module's own work: copying the state in and
  out and each body's `enabled` (with the sines and cosines JavaScript
  computed for it then), about a quarter of the kernel's time under Node (the bodies' flags alone
  three tenths of that). The module could name the few bodies a tick can
  reach, so that only theirs are read; on these maps that would save under
  1% of the step.
- The replay's other work stays in JavaScript: each weapon's state captured
  and restored, recoil, and the crowd push against the other soldiers. Half
  of the page's simulation phase is taking in the world's packets, which this
  leaves as it was.
- Droidekas walk by their own clips and collision pass (src/droideka.js),
  still in JavaScript, as are the hulls.
- The kernel stays behind `movement=wasm`; making it the default goes with
  the ray kernel's.
