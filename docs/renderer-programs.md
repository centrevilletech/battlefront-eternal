# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family owns twelve stable full-screen and battle HUD
programs. The FXAA source license is retained in `shaders/post-hud/NOTICE`.
Those passes register Rust source directly. They have no source switch,
compiled-three-program bridge, or shader fallback. Registration errors throw a
message naming the variant. Unported shader families still borrow three programs.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32,
sampler3D 33).
Order in the uniform object defines slot order. Include every active input;
inactive declared inputs may remain. The linker validates active uniforms and
attribute locations against the declaration. Unknown keys, incompatible layouts
and link failures are errors.

`programs::sources` emits GLSL ES 3.00 with explicit high precision and the
compatibility vocabulary the bodies use. It injects no lighting, colour-space or
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

The shared interface Batch also serves menu surfaces, the spawn holomap and HUD
pointer/overlay draws outside the battle HUD phase. Those real consumers still
need three's quad material. `interface/batch-shaders.js` is generated from the
canonical HUD bodies for them; it cannot select or replace the battle HUD program.
Remove it when those interface draws acquire their own Rust phase.

# Parallel shader families

Add a separate `shaders/<family>/manifest.json` and source files. Suggested
namespaces are `world` (world/terrain/foliage), `actors` (actor lighting/shadows),
and `effects` (effects/water/sky). Use stable string keys, not numeric allocations.
The generator discovers family directories, validates uniqueness and emits a
sorted catalog. Regenerate after merging; do not merge generated output by hand.
A manifest row declares `vertex`, `fragment`, `attributes` pairs and `uniforms`.
Use the post-hud manifest as the concrete schema example.

Registration currently accepts only Plain programs. Families needing instance,
frame or shadow source rewrites must extend the descriptor/compiler explicitly;
do not route instanced programs through Plain. Shader registration does not alter
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


# Compilation and caches

Owned source removes the dependency on three's first draw and exposes the finite
variants before drawing. It permits loading-screen prewarming, paced work and
content-addressed source/layout caches. Covered post draws now have no three
shader material to compile. The main HUD registers Rust directly; unrelated UI
quad consumers still compile their material. Unported families retain their
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
