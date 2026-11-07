# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family owns twelve stable full-screen and battle HUD
programs. The `world` family owns static near/far geometry, terrain, foliage
and canopy programs, including their Enhanced variants. The FXAA source license is retained in `shaders/post-hud/NOTICE`.
The `interface` family owns menu quads, model/skin/vertex-colour previews,
spawn rooms, the projector, motes and holomap surfaces. Its skinning source
license is retained in `shaders/interface/NOTICE`.
Those passes register Rust source directly. They have no source switch,
compiled-three-program bridge, or shader fallback. Registration errors throw a
message naming the variant. Line utilities still use three's compiler on first use.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32,
sampler3D 33, sampler2DShadow 34, samplerCube 35). Arrays declare `{ "type": N, "size": N }`
instead of a numeric type, using the base uniform name without `[0]`.
Order in the uniform object defines slot order. Include every active input;
inactive declared inputs may remain. The linker validates active uniforms and
attribute locations against the declaration. Unknown keys, incompatible layouts
and link failures are errors.

`programs::sources` emits GLSL ES 3.00 with explicit high precision and the
compatibility vocabulary the bodies use in `body` mode; `expanded` mode keeps
the declared version and vocabulary. It injects no lighting, colour-space or
tone-map chunks. Source and arithmetic belong to Rust. Run
`node tools/render-shaders.mjs` to regenerate `program_catalog.rs` and the
browser's `render-programs.js` layout/hash metadata. The latter contains no shader
bodies. `npm run rust:check` checks generated outputs and WASM. Do not edit them.

`BfeRenderer.add_owned_program(key)` compiles and validates the declared variant
and returns a context-local program ID. Repeated registration returns the same
ID. `ownedProgram(core, key, attributes)` returns `{id, uniforms, attributes}`
to passes. `ownedFullScreenPass(key, uniforms)` creates a three Material carrying
uniform values and the key, without browser shader source. Registration requires
no prior three draw. `core.shaderPrograms` records key, source/layout SHA-256 and
registration duration. Post targets are explicitly initialized before adoption.

The battle post chain publishes generated facts to `post_chain.rs`: settings,
render/output dimensions, vehicle blur and resource IDs. Rust chooses the
ordered programs, targets and uniform values, owns camera-cut/jitter/history
state, and commits history only after the complete draw succeeds. The browser
retains target and camera shells; it applies the published camera adjustment.
There are no post-step material shells or browser post-step arrays. Paced
loading registers all eleven post variants before battle drawing. The boundary
comes from `post-chain-layout.json` and `tools/render-post-layout.mjs`.

The loaded battle publishes semantic view packets through the generated
`frame-layout.json` boundary. `frame_plan.rs` owns cloud/probe cadence, mirror
holds, cascade refreshes and the complete target/clear/copy/resolve schedule.
`Renderer::battle_frame` borrows the existing resource bank and executes that
schedule without browser scene or drawing callbacks. Mirror packets retain the
previous shadow inputs; the main shadow refresh follows their execution.

Scene preparation still reads three cameras, scene transforms and descriptors.
The host snapshots only live record/bone/FX prefixes into retained native view
storage before another camera overwrites the working inputs. Dynamic interface
uploads belong to their UI packet. Lines join the existing native transparent
sorter, alongside the sorted particle and fountain copy commands. The far image
remains after opaque depth. Native post completion commits temporal history only
after the planned chain succeeds; clipped frames invalidate that history.

Interface Batch descriptors carry texture/mask values without Three materials
or browser shader text. Menu and spawn scenes publish generated draw records to
`interface_scene.rs`; Rust sorts their opaque/transparent groups and applies
depth, blending, culling and indexed/point draws. `interface/quad` shares the
canonical Rust HUD shader files with a separate context-local program key, so
the two passes cannot invalidate each other's matrix cache. The record layout
comes from `interface-layout.json` through `tools/render-interface-layout.mjs`.

Dynamic interface vertices have persistent WASM byte mirrors. Changed attributes
upload into retained GPU buffers without marshalling a new byte slice each
frame. Context reconstruction replays catalog keys and geometry bindings, moves
the current mirrors and uploads their bytes before drawing resumes. Three
scene, camera, skeleton, geometry and texture descriptors remain presentation
inputs. The browser's scene draw entry point is guarded for every surface.

# Parallel shader families

Add a separate `shaders/<family>/manifest.json` and source files. Suggested
namespaces are `world` (world/terrain/foliage), `actors` (actor lighting/shadows),
and `effects` (effects/water/sky). Use stable string keys, not numeric allocations.
The generator discovers family directories, validates uniqueness and emits a
sorted catalog. Regenerate after merging; do not merge generated output by hand.
A manifest row declares `vertex`, `fragment`, `attributes` pairs and `uniforms`.
Use the post-hud manifest as the concrete schema example.

Registration dispatches explicitly by descriptor kind. Actor receivers use
Skinned or Rigid; casters and per-object flashes use Plain (a caster's matrices
and bones are its own uniforms, so it needs no rewrite).
Manifest rows accept `Plain` (the default), `World`, `Skinned`, `Rigid`, `Effect` and `Motion` kinds.
World uses the existing frame/shadow block rewrite and may declare `lamps: true`
for the lamp-table rewrite. `source: "expanded"` supplies complete GLSL ES 3.00
sources, including the version and precision declarations; the default `body`
uses the post/HUD compatibility prefix. Expanded arithmetic is preserved before
kind-specific input rewrites. Reflection validates standalone inputs against the
manifest; block members belong to the shared frame/shadow layout.
Shader registration does not alter
`core.rs`, `post.rs` or `hud.rs` phase records: context-local resource handles,
f64 phase inputs, five view blocks and BfeFrame/BfeShadow layouts remain their
own contracts.

Effect uses the shared frame block and identity model. Motion uses its motion
row; Skinned uses the bone/instance row for effects overlays. Kind-specific
injected inputs are supplied by the matching phase.

A program whose only difference from another is the shadow-map defines is a
shadowed twin: its row declares `shadowOf: {key, vertexLine, fragmentLine}`
instead of source files, and the generator and the Rust registry both build its
text from the base source with those two defines inserted at the recorded lines.
Each such pair therefore stores one copy of the text, and the generated layout
gives the base a `shadowTwin` key that the page uses to pick the shadowed
program. A test requires every selector to exist with shadows on and off. Version resource/schema changes independently of source keys.
Caches belong to one GPU context and must be discarded with it. Context lifecycle
and renderer-wide error policy belong to the renderer lifecycle packet.

# Reusable pixel corpus

Run `node tools/renderer-corpus.mjs --runner EXECUTABLE --dist DIST --against
REFERENCE_DIST --out OUTPUT --jobs N`. The runner receives
`DIST CASE_ID CASE_OUTPUT REFERENCE_DIST`. It must run three separate application
sessions in that case directory's `reference`, `control` and `candidate`
subdirectories, invoking `node tools/renderer-corpus/capture.mjs` for each. The
first two sessions use the frozen reference; the last uses the candidate. Each
session uses the same immutable case query and settings and must retain draws.
Build both distributions with `--minify false`. The harness isolates object UUID
randomness from procedural foliage/effect randomness, so deleting helper objects
does not change the sampled world. Audio scheduling uses a separate stream too. It fails if that isolation cannot be verified.
The worker is stopped and advanced to authority tick 60 before cosmetic warmup;
its initial start acknowledgement can otherwise race by one tick. Captured ticks
must still match exactly between sessions.

Set `CDP_PORT`, `BFE_CORPUS_CASE`, `BFE_CORPUS_ROLE` (`reference`, `control` or
`candidate`), `OUT` and `BFE_SETTINGS_FILE` for each session. The manifest's
context-loss case (`restore-nab2`, `contextLoss`) exercises restoration. `BFE_APP_URL` defaults to the
desktop application.
The runner must propagate capture failures. Use Node with built-in fetch and
WebSocket. No content is distributed with the corpus.

Comparison is exact for every byte. The only exceptions are the named cases in
`tools/renderer-corpus/expectations.json`, each with its measured pixel count,
channel delta and the explanation of where the difference comes from. A bound
applies to that case's candidate only: its control stays exact, and a larger
count or delta fails. The summary records the expectations file's hash.

Post-policy captures record scalar/resource facts and temporal state before
selection, plus the initial scene, depth, velocity and history samplers. Native
replay must reproduce every active plan word exactly before executing the
whole chain. Frozen self/control and candidate outputs must then match exactly;
per-step post/HUD replay remains an independent downstream check. Separate
fresh-launch frame comparisons retain their own determinism limits.
`post-manifest.json` adds continuous temporal sequences, scale changes in both
directions, direct bilinear and FXAA choices, strong/tiny vehicle blur, mixed
resize/settings transitions and deliberate context loss/restoration. The last
case requires one observed loss/restore pair and the same NVIDIA OpenGL backend
before and after it. The readback framebuffer is recreated independently of the
renderer's managed resources.

The manifest freezes all 17 maps at Classic/Balanced/High, valid first-person
player queries, requested 2x MSAA, four walking/turning points and four history
frames. Supplemental cases cover Ultra water and mirrors, upscaling and context
restoration. `--cases id,id` is a smoke, not the full matrix. Captures include
actual ticks, compressed RGBA, context events, material-state census, Rust draw
counts and the battle draw guard's count. Candidate renderer errors are fatal.

Each run uses seeded page/worker randomness and simulation lockstep. Before
entering lockstep, the local host stops and publishes one full snapshot through
its existing resync/advance API, so stationary actors share the worker's tick.
Each point records far/scene inputs, temporal velocity/motion/history where enabled, planar
reflection stages, post output and the final weapon/HUD frame. Half-float stages
are compared as half-float bits. All comparisons include RGBA. Mismatched ticks,
layouts, missing mirrors, blank frames, absent Rust draws, context loss, GPU
process deaths and GL errors invalidate the case. Transient stage buffers are
deleted after comparison; compact metrics and run metadata remain.


# Whole-build pixel gate

`node tools/build-gate.mjs --reference DIST --candidate DIST --out DIR` compares
a candidate desktop build with a reference build over every map at
Classic, Balanced and High: on foot in first person with the HUD (`foot-*`), in a
vehicle in the chase view (`vehicle-*`) and the cockpit (`cockpit-*`), and on the
front end's own screens (`shell-*`). A vehicle case walks the player to the
nearest hull within 120 m of the spawn and presses E; a map with no hull in
reach is reported `skipped`, not failed. `--list` prints the case ids; `--kind`,
`--map`, `--preset` and `--cases` select a subset, which the summary marks as
incomplete.

Fresh whole-game sessions are not pixel identical, so a case records the
reference's renderer inputs once (the post, HUD and world stage inputs of the
reusable corpus) and replays them in the reference, in a repeat session of the
reference and in the candidate. Those replays must be RGBA exact, with no
tolerance. The finished frame is read in all three sessions as well: the repeat
shows how much of a frame differs between two runs of one build, and the
candidate's pixels beyond that need a cause: the case's `frame.verdict` is `exact`,
`within-noise` (the candidate's visible differences, over 16 levels in a channel,
are within 1.5 times the repeat's plus 0.1% of the frame) or `beyond-noise`. A build with no stage interface
(a three.js-only build) can only be compared this way, and the case reports
`frame-only`. Each case also records the world state it drew (tick, player and
camera position, live units) so a frame difference can be told from a world
difference. A difference image (red: changed where the repeat did not; yellow:
changed where the repeat is noisy too) is written next to the result.

Build both distributions with `--minify false` for the cleanest comparison. A
minified build cannot tell the object-id stream of the seeded randomness from
the one that scatters leaves, flowers and grass (the harness finds it by the
`generateUUID` function name), so two runs of the same minified build scatter
differently and a fifth to a half of the frame can differ between them. Two runs of
an unminified build agree to the pixel, and the gate's frame comparison is then
exact for equal builds. The chase-view vehicle cases need a minified build until
an unminified one draws a third-person soldier.

`--wrap` names the command that runs one case where the GPU is, and `--jobs` how
many run at once. The case runs `tools/build-gate/run-case.mjs` there, which
starts the application with the `BFE_GATE_START` command (given the dist as
`$1`, returning once the application answers on `CDP_PORT`), stops it with
`BFE_GATE_STOP`, and runs the three sessions in turn on the same display, so
reference, repeat and candidate always share one GPU and driver. Use the
OpenGL ANGLE backend: Vulkan replays are not exact. Raw pixels and inputs
(`BFE_GATE_RAW`) are removed after the comparison and must not be kept or
committed.


# Compilation and caches

Owned source removes the dependency on three's first draw and exposes the finite
variants before drawing. It permits loading-screen prewarming, paced work and
content-addressed source/layout caches. Covered post draws now have no three
shader material to compile. HUD and interface quads register Rust directly. Unported families retain their
preparation work, so source ownership alone does not remove the long loading task.

The current cache avoids repeated linking in one context. In an earlier warm
GPU-driver diagnostic, first registration of all twelve variants took 121.970 ms
and repeating the keys took 0.165 ms, returning the same IDs. In fresh default-path
applications, registering ten unused variants with two already cached took
2.925–5.941 seconds; repeating all twelve took 0.075–0.145 ms. Driver-cache state
and queued status waits matter. Neither probe measures persistent binary reuse.

Local interleaved frozen-reference/candidate/candidate/reference load diagnostics
used Classic settings, draws enabled, 1600x900, three preparation draws, twelve
seconds of simulation warmup and retained driver caches. Reference preparation
was 144.014/153.056 s; candidate was 128.816/151.910 s. Both linked 134 programs.
Worst main-thread tasks were 86.817/104.908 s versus 81.682/96.096 s. These
software-GPU rounds do not establish a startup improvement or reproduce the
reported 24-second loading configuration. No laptop speedup is claimed.
Compile/link API calls queued work in 2.4–4.2 ms; combined shader/program status
waits took 77.684–97.170 s. Owning source permits earlier or paced compilation,
but the remaining driver waits still need that scheduling work.

A persistent cache needs source/schema hashes, variant flags, backend and GPU/driver
identity. WebGL offers no portable program-binary persistence; browser/ANGLE disk
caches are opportunistic. Native binaries require fingerprint validation and
recompilation from the owned Rust source when invalid. Ownership cannot eliminate
first-driver compilation or ANGLE translation.

`tools/renderer-corpus/load.mjs` records compile/link/status/reflection calls,
source submissions, long tasks, preparation/ready times and registration durations
on the default path. `BFE_SHADER_CACHE_PROBE=1` repeats all registered keys to
measure context-local ID reuse separately. Compare fresh applications against a
frozen reference in interleaved local rounds, recording driver-cache state. Farm
timings are correctness diagnostics, not slow-PC performance measurements.

Manifest rows may declare a `kind` (listed above), `source` (`body` or `expanded`), and boolean
`lamps`. Defaults preserve the post/HUD body format and Plain kind. Every other
kind requires expanded GLSL ES 3.00, including its version and precision header;
no prefix is added. The compiler applies the selected kind's existing input
rewrite. Skinned and
rigid receivers use their instance rows and shadow blocks. Source hashes include
these compiler options. Individual material-uniform validation excludes block
members, whose layout belongs to the frame/shadow schema.

A source path may also be an ordered array of paths. Rust concatenates those
parts before compilation; the generator hashes the same complete GLSL text.
Shared chunks therefore occupy one copy in the module without depending on
three's chunk registry or putting source text in browser metadata. Actor source
parts retain the expanded shader's expression and declaration order. The actor
chunks are derived files: `python3 tools/actor-shader-chunks.py --join DIR`
writes each program's complete sources for reading or editing, `--factor DIR`
rewrites the chunks and the manifest's lists from them, and `--check` (run by
`tests/actor-chunks.test.js`) fails unless factoring the joined sources gives the
checked-in chunks back.

An object no Rust pass can claim (the near and mirror walks, a shadow cascade's
casters) is not drawn for that frame and is reported once, with its name, type
and material key, through the renderer's error list and the console
(`RustRender.skip`). An actor material no program variant covers is skipped the
same way and reported once per variant (`core.missingVariants` counts the
objects); the census tests keep that unreachable. A program the GPU rejects and
an incomplete configuration remain fatal.

Actor, rig and model-part material shells carry flags and uniform values. Their closed selector uses
skin mode, material defines, cascade count and lamp mode. Unknown combinations
name the selector in that report; no three program is compiled for an actor shell.
First-person flashes and shock overlays are the effects family's (below).
Bodies, rigs and the remaining view parts use Skinned or Rigid programs.
Caster descriptors (the `actors/depth-*` keys) preserve per-caster matrices, cutout sampling and bones.

The actor corpus can be selected with
`node tools/renderer-actor-corpus.mjs`. Its runner invokes `actor-capture.mjs`
and `actor-compare.mjs` in `tools/renderer-corpus/`. It crosses all maps and
quality presets with every shadow setting, and adds droideka, dark trooper and
AI hero views, heated station wrecks and skinned shock overlays. First-person
flash cases invoke the real presentation envelope. Hero cases use eligible player
classes and observe the real AI hero. The census tool records expanded sources and reflection for every linked
program, including warm-up programs that are subsequently disposed.

Actor replay freezes geometry, poses, frame blocks, material records, sampler
levels and sampler state. Receiver variants draw separately into an isolated
actor target using the regular frame or view API; world draws are excluded.
Mirror replay retains the mirror frame and its far image. Per-object view draws
and live cascade casters have separate replay records. Each caster variant also
replays into its own cleared depth target, so other casters cannot hide its output.
Reports distinguish registration, replay and visible-pixel coverage. A synthetic
skinned caster with a half-transparent mask isolates the skinned cutout depth
program on maps whose character textures are opaque. Reference self-replay,
candidate replay and repeat-reference replay must all match exact RGBA values
and the same record layouts. Depth packing is lossless and verifies the complete
RGBA channel pattern before retaining one channel. These actor-stage checks do
not establish whole-frame equality across independent applications.

Dissolve cases apply the real droideka death-presentation method to a live
battle model and retain its resulting material and heat inputs. They exercise
shader behavior while the seeded simulation continues. They do not test combat
death scheduling. The same cases spawn real debris models for each heated
material flag set, with the factory supplying their heat records. Startup stops the worker before lockstep and synchronizes the
page with an authoritative entity publication; a missed throttled publication
requests full snapshots on bounded ordinary startup ticks, with the same control
envelope as normal lockstep. The absolute warm-up tick
and all capture ticks remain fixed.

# Effects programs

The effects catalog owns the expanded particle, beam, shield, weather, volume,
fountain, water, ocean, sky and motion stages. Its NOTICE retains the license
for the expanded three chunks. Material shells carry uniform values and raster
state. The effects, water and motion passes require catalog registration; they
have no compiled-material bridge. World and actor programs remain separate.

The loading screen links only the effects variants the current settings can draw
(shadowed twins with shadows on, their bases with shadows off), one at a time
with a settle between links. Changing the shadow setting queues the other set,
linked one program per frame.

The catalog distinguishes camera transport as well as source defines. Near
batches use Effect, model draws use World, and motion proxies use Motion.
Shock overlays use Skinned with the existing instance and bone rows. Water and
first-person overlays retain Plain camera uniforms. First-person arc geometry
keeps explicit instanced attributes; the view pass uploads its camera uniforms
and draws that batch at its sorted position. Separate glow and beam keys keep
the passes' uniform caches independent.

`tools/renderer-corpus/census.mjs` retains linked sources, including preparation
programs disposed before the final battle frame. Run
`tools/renderer-corpus/coverage.mjs CENSUS FAMILY SUMMARY` to audit the captured
variants against a family manifest. Unowned effect variants remain named gaps.

For GPU draw replay, build the native `shader-catalog` example and convert its
length-delimited output with `tools/renderer-corpus/shader-index.mjs`. Set
`BFE_SHADER_INDEX` to that index while running the corpus. The index contains
source hashes, kinds and attribute layouts; shader bodies stay in Rust. The
capture verifies full SHA-256 hashes against the index.

Effects fixtures record vertex/index buffers, uniforms, uniform blocks, sampler
state, cube/3D/mip texture contents and raster state from actual draws. Each
reference draw establishes an isolated output over fixed color and depth. A
second fixture adds fixed depth occluders. Fresh resources must reproduce both
outputs exactly in reference self replay, candidate and repeat reference. The
candidate links only `add_owned_program(key)` and submits the recorded draw
through Rust. Diagnostic reference compilation uses the frozen stage source.
`tools/renderer-corpus/replay-draws.mjs` replays saved draw fixtures in a fresh
context without loading another battle. These fixtures verify shader behavior;
the frozen battle census and phase tests cover registration and input publishing.

Use `--manifest tools/renderer-corpus/effects.json` for deferred effect scenarios.
It inherits the frozen quality presets and adds underwater views, jetpack and
shield inputs, Classic lightning and Enhanced first-person shock discharge.
`BFE_CORPUS_MANIFEST` passes the selected manifest to a runner. Captures retain
linked warm-program inventories as well as live material programs, so disposed
preparation programs remain available to the ownership audit.

The local load tool reports compile/link counts during sixty first-use frames
and render-submission mean, median, p95 and maximum times. Run fresh reference,
candidate and repeat-reference applications on the same local slot; correctness
farm timings do not establish CPU performance.

After an interrupted or invalid run, `--resume` reuses complete passing case
reports only when the output summary names the same frozen builds and manifest.
Failed or missing cases run again. Use `--rerun CASE,...` with `--resume` to
recapture cases affected by an input-capture correction, including cases whose
previous outputs happened to match.

Uniform-array capture reads every element separately; WebGL's `getUniform`
returns only the requested element. For a frozen draw with standalone camera
uniforms, replay can copy those exact float32 values into the owned World
block at reflected offsets. Object matrices remain unchanged. Both the
expanded input source and the linked owned stages require exact SHA-256 matches.

`effects-bodies.json` adds third-person shock cases to exercise the visible
skinned overlay as well as the first-person arc cases in `effects.json`.
`effects-blades.json` covers saber materials in all three presets and the
third-person draw. Command-post logos, build holograms and blades use the
source-free basic material shells. Native draws run their existing facing
hooks before publishing matrices, including the view's motion mask.
The saber fixture uses the existing unit presentation factory in an infantry
battle; heroes are outside the local server's playable roster. Stationary
players enter corpus lockstep after one ordinary movement command if their
initial entity publication has left the page clock behind the local worker.
The page and worker clocks must match before any capture.
`effects-shields.json` activates the Droideka secondary shield in every preset
so shield pixels are checked separately from the rolling-dust cases.
`effects-noshadow.json` repeats the Balanced and High nab2 and kam1 (Enhanced ocean), mirror,
shock, arc, jetpack, Droideka and shield cases with shadows off, so every effect that
has a shadowed twin also draws through its unshadowed program.
`effects-trails.json` checks Enhanced rolling dust with shadows disabled; its
expanded stages also appear in the Classic prepared-body compilation census.

The catalog example's `--instance-layout` output exports Motion and Skinned row fields from
Rust's constants. Pass that JSON as the optional third file argument to
`shader-index.mjs`. Standalone motion inputs copy their recorded object matrices
into that row; their bone textures retain the same values and use zero row bases.
Standalone skinned inputs use the same row plus the reflected camera block;
the replay copies the original normal and binding matrices without recomputing them.
Material fields such as shock intensity and inflation use the row loads in the
hash-verified native stages as their layout; their captured values stay unchanged.
When identical stages have different link-time attribute locations, replay
rebinds each captured input by name, preserving matrix-column offsets and values.

# Frozen world shader inputs

`tools/renderer-world-corpus.mjs` runs the source gate independently of the
renderer lifecycle corpus. Its runner receives candidate distribution, case id,
output directory and frozen reference distribution. Launch reference, candidate
and repeat reference in that order, invoking
`tools/renderer-corpus/world-capture.mjs` with the corresponding
`BFE_CORPUS_ROLE`; then invoke `world-compare.mjs` with output directory and case
id. The world manifest includes the 51 map/preset cases, three-cascade shadows,
a close canopy view, water and scale cases. Shortened captures cannot pass
completeness.

The reference records actual default Rust submissions: vertex/index and uniform
block bytes, standalone uniforms, sampler mip texels or compressed blocks,
filter/wrap/comparison/anisotropy parameters, raster state and multi-draw ranges.
Source replay uses Rust-registered linked programs and fresh targets with the
same sample count. Depth inputs retain their original depth format. This checks
world/far, terrain, foliage, canopy and world-transparent arithmetic and alpha-to-coverage without
incoming colour/depth history from unrelated draws. Post/HUD replay retains its
phase records. Reference self-replay, candidate replay and repeat-reference
replay must all match every RGBA channel and half-float bit exactly. Shader and
layout hashes accompany the reports; transient buffers are deleted after
comparison. Whole-frame differences remain a separate verdict.

World source prefixes are explicit `vertexPrefix`/`fragmentPrefix` manifest
fields. Rust concatenates each prefix with a shared expanded body at registration;
the generator hashes the complete expanded text. This shares identical bodies
without changing shader bytes or emitting browser GLSL. World resource shells
never enter three's compiler. Scene features select an exact declared key;
unknown combinations fail rather than choosing another variant. Map programs
are prewarmed before preparation draws. The world shader license is retained in
`shaders/world/NOTICE`.

Transparent source cases aim at authored triangles for each material feature
set. Their simulation advances between points without drawing; settle and
history frames produce the sampled inputs. They capture both transparent-list
and segmented native submissions, including the mirror list. A transparent
case must replay an actual world-transparent submission. Unbound samplers are
recorded as null bindings. First-use source submissions invalidate the candidate;
the frozen reference retains its original lazy compilation diagnostics.
