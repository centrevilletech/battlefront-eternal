# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family contains the stable full-screen and HUD shaders.
Other draws still use the compiled-source bridge. Rust is required for all battle draws. Unavailable modules and rejected
programs stop loading with the GPU name and original error.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32).
Order in the uniform object defines slot order. Include every active input;
inactive declared inputs may remain. The linker validates active uniforms and
attribute locations against the declaration. Unknown keys, incompatible layouts
and link failures are refused, never mapped to another variant.

`programs::sources` emits GLSL ES 3.00 with explicit high precision and only the
compatibility vocabulary the bodies use. It injects no lighting, colour-space or
tone-map chunks. Source and arithmetic belong to Rust. `node tools/render-shaders.mjs`
emits `program_catalog.rs` and the browser's `render-shaders.js` fallback from the
same manifests and files. `npm run rust:check` checks both outputs as well as the
WASM. Do not edit generated files.

`BfeRenderer.add_owned_program(key)` compiles and validates the declared variant
and returns a context-local program ID. Repeated registration returns the same
ID. `ownedProgram(core, material, attributes)` registers materials tagged with
`material.userData.bfeProgram`, returning the existing `{id, uniforms, attributes}`
contract to passes. `ownedFullScreenPass(key, uniforms)` creates the transitional
three material with the same source. Registration no longer requires three to
have drawn the material first. `core.shaderPrograms` reports the key, source and
layout SHA-256, registration duration and acceptance. The developer-only
`?rustShaderBridge` or `core.ownedShaderSources = false` selects the compiled-source bridge for A/B
checks; it is not a player setting.

Independent shader families add their own directory and manifest. Suggested
namespaces are `world`, `actors` and `effects`; keep world/terrain/foliage,
actor lighting/shadows, and effects/water/sky in their respective directories.
Regenerate both catalogs after merging. No shared numeric program-kind or pass
allocation is needed for source keys. Families needing instance/frame/shadow
rewrites must extend the source descriptor and compiler explicitly before
registration; the current descriptor deliberately supports only Plain programs.
Do not silently feed an instanced shader through the Plain contract.

Frame and resource contracts stay those of `core.rs`, `post.rs`, `hud.rs` and the
existing pass bindings: numeric context-local program/geometry/texture handles,
f64 phase inputs, the five view blocks, and the existing BfeFrame/BfeShadow
layouts. This change does not change draw order, texture ownership, target
resolve rules or temporal commits. Future frame/resource schema changes must be
versioned independently of shader keys. Context restoration recreates GPU programs and buffers from the Rust resource
journal while keeping logical handles stable. Three reuploads its textures.

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
does not change the sampled world. It fails if that isolation cannot be verified.
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
also require exact RGB pixels, dimensions and ticks. There is no per-count or
noise-envelope allowance. Raw captures stay outside the repository and should
be removed after recording results.

`tools/renderer-corpus/load.mjs` uses the same loader and case settings for
local load diagnostics. Set `BFE_SHADER_BRIDGE=1` for the old compiled-source
path. It records GL compile/link/status/reflection and initial draw API calls,
source submissions versus unique source texts, long tasks, preparation and ready
times, and per-program registration durations. `BFE_SHADER_CACHE_PROBE=1` additionally
registers all declared variants and repeats each registration after loading,
recording context-local ID reuse separately from startup work. Use fresh applications in
interleaved rounds; record whether driver caches were retained. It deliberately
does not run through the farm performance gate.
