# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family owns twelve stable full-screen and battle HUD
programs. Those passes register Rust source directly. They have no source switch,
compiled-three-program bridge, or shader fallback. Registration errors throw a
message naming the variant. Unported shader families still borrow three programs.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32).
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
and `BFE_GPU_LOG` for the application log. `BFE_APP_URL` defaults to the desktop
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

Exact equality is reported separately. The repeat reference provides a
per-channel, per-pixel amplitude envelope; a difference outside it fails. A noisy
control never grants a blanket pixel budget. Inspect geo1 smoke/flames, bes2
steam, kam1 rain/lightning and rhn1 dry ice in particular. The corpus visits
representative states, not every possible effect or camera position.

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

The current cache avoids repeated linking in one context. In local software-GPU
diagnostics, first registration of all twelve variants in a warm driver/context
took 121.970 ms; repeating those keys took 0.165 ms and returned the same IDs.
This is not a cold-start or persistent-binary measurement. Interleaved full-load
rounds did not establish a startup improvement: bridge preparation was
129.929/139.614 s and owned preparation 142.394/143.518 s. Driver caches were
retained and software-GPU contention limits that comparison. No laptop speedup
is claimed. Compile/link enqueue time was below 3 ms; shader/program status waits
dominated (one diagnostic pair: 68.449/9.457 s versus 59.178/8.724 s).

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
