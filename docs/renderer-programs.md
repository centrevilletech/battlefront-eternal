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
message naming the variant. Unported shader families still borrow three programs.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32,
sampler3D 33, sampler2DShadow 34). Arrays declare `{ "type": N, "size": N }`
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

Manifest rows accept `kind: "Plain"` (the default) or `kind: "World"`.
World uses the existing frame/shadow block rewrite and may declare `lamps: true`
for the lamp-table rewrite. `source: "expanded"` supplies complete GLSL ES 3.00
sources, including the version and precision declarations; the default `body`
uses the post/HUD compatibility prefix. Expanded arithmetic is preserved before
kind-specific input rewrites. Reflection validates standalone inputs against the
manifest; block members belong to the shared frame/shadow layout. Families
needing instance rewrites must extend the descriptor explicitly; do not route
instanced programs through Plain. Shader registration does not alter
`core.rs`, `post.rs` or `hud.rs` phase records: context-local resource handles,
f64 phase inputs, five view blocks and BfeFrame/BfeShadow layouts remain their
own contracts. Version resource/schema changes independently of source keys.
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

Set `CDP_PORT`, `BFE_CORPUS_CASE`, `OUT` and `BFE_SETTINGS_FILE` for each session.
Set `BFE_CANDIDATE=1` only for the candidate, and `BFE_FORCE_RESTORE=1` to exercise
the context-loss case on it. `BFE_APP_URL` defaults to the desktop application.
The runner must propagate capture failures. Use Node with built-in fetch and
WebSocket. No content is distributed with the corpus.

Comparison is exact for every byte. The only exceptions are the named cases in
`tools/renderer-corpus/expectations.json`, each with its measured pixel count,
channel delta and the explanation of where the difference comes from. A bound
applies to that case's candidate only: its control stays exact, and a larger
count or delta fails. The summary records the expectations file's hash.

The manifest freezes all 17 maps at Classic/Balanced/High, valid first-person
player queries, requested 2x MSAA, four walking/turning points and four history
frames. Supplemental cases cover Ultra water and mirrors, upscaling and context
restoration. `--cases id,id` is a smoke, not the full matrix. Captures include
actual ticks, compressed RGBA, context events, material-state census, Rust draw
counts and the battle draw guard's count. Candidate renderer errors are fatal.

The page and workers use seeded randomness. Capture-time lamp selection uses a
fixed advancing clock. Reference/control must be pixel-exact across separate
sessions; noisy controls fail and must be investigated. Candidate comparisons
also require exact RGBA pixels, dimensions and ticks. There is no per-count or
noise-envelope allowance. Raw captures stay outside the repository and should
be removed after recording results.


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
