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

# Reusable frozen-build corpus

Run `node tools/renderer-corpus.mjs --runner EXECUTABLE --against REFERENCE_DIST
--dist CANDIDATE_DIST --out OUTPUT --jobs N`. The runner receives
`CANDIDATE_DIST CASE_ID OUTPUT REFERENCE_DIST`. For each case it launches fresh
applications in order: reference, candidate, repeat reference. In each application
run `node tools/renderer-corpus/capture.mjs` with `BFE_CORPUS_ROLE` set respectively
to `reference`, `candidate`, `control`. Then run
`node tools/renderer-corpus/compare.mjs OUTPUT CASE_ID` and return its exit status.
No query or environment option selects a shader implementation.

Set `CDP_PORT`, `BFE_CORPUS_CASE`, `OUT`, `BFE_SETTINGS_FILE` for desktop settings,
and `BFE_GPU_LOG` for the application log. `BFE_CORPUS_RAW` optionally places
transient buffers on local scratch storage instead of the report directory. `BFE_APP_URL` defaults to the desktop
app URL. Use Node with built-in fetch and WebSocket. Launch at 1600x900 with draws
enabled and `WARM=12`, `RENDER_WARM=3`. Freeze both distributions before the run.
`--cases id,id` is an iteration smoke, not the complete gate. The summary records
manifest SHA-256, build paths, missing program variants and completeness.

The version 2 manifest has every one of the 17 maps in Classic/Balanced/High,
requested 2x MSAA, four walking/turning/firing capture points and four history
frames after two settling frames. Additional cases cover Ultra water on ocean
and mirror maps, bilinear/spatial scaling and fast FXAA. Actual driver sample
counts are reported. Deliberate context-loss behavior is covered by the renderer
lifecycle gate, independently of this shader-source comparison.

Each run uses seeded page/worker randomness and simulation lockstep. Each point
records far/scene inputs, temporal velocity/motion/history where enabled, planar
reflection stages, post output and the final weapon/HUD frame. Half-float stages
are compared as half-float bits. All comparisons include RGBA. Mismatched ticks,
layouts, missing mirrors, blank frames, absent Rust draws, context loss, GPU
process deaths and GL errors invalidate the case. Transient stage buffers are
deleted after comparison; compact metrics and run metadata remain.

The exact shader gate replays recorded reference inputs through each build's
default Rust API. It records each post step's f64 inputs, sampler texels and
output, plus HUD vertex mirrors, camera/group inputs and pre-HUD canvas. The
reference first replays its own capture and must reproduce it exactly; then the
candidate must reproduce the same outputs exactly. Texture levels and sampler
parameters are preserved. Depth is sampled losslessly into float texels; the
reference self-replay checks this representation. Every channel and half-float
bit must match. This isolates shader behavior from differences between separate
live battles. Transient payloads are deleted after comparison.

The whole-frame baseline verdict remains separate. Independent launches can
differ even with matching simulation ticks, so frame exactness must not be
inferred from shader exactness. The repeat reference provides a
per-channel, per-pixel amplitude envelope; a difference outside it fails the
whole-frame baseline. A noisy
control never grants a blanket pixel budget. Inspect geo1 smoke/flames, bes2
steam, kam1 rain/lightning and rhn1 dry ice in particular. The corpus visits
representative states, not every possible effect or camera position.

`BFE_CORPUS_POINTS=1` is a development capture and cannot pass completeness.

The version 1 pre-deletion gate paired the compiled-three and owned programs on
identical inputs for each step, the whole post chain and HUD. That frozen evidence
remains an external reference; the runtime twin and switch are deleted. Synthetic
phase-record goldens and registration tests preserve coverage in CI.

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
