# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family contains the stable full-screen and HUD shaders.
Other draws still use the compiled-source bridge. Three remains the fallback for
an unavailable renderer, an explicitly disabled pass, or a rejected program.

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
versioned independently of shader keys. Cache entries must not survive context
loss: the bridge disables itself and three rebuilds its own resources.

# Reusable pixel corpus

Run `node tools/renderer-corpus.mjs --runner EXECUTABLE --dist DIST --out OUTPUT
--jobs N`. The executable receives `DIST CASE_ID OUTPUT` and must launch an
isolated application with draws enabled, then run
`node tools/renderer-corpus/capture.mjs`. Set `CDP_PORT`, `BFE_CORPUS_CASE`, `OUT`,
`LABEL` (the case ID), `BFE_SETTINGS_FILE` when the desktop settings need writing,
and `BFE_GPU_LOG` for the application log. `BFE_APP_URL` defaults to the desktop
app URL. The runner must exit with the capture's exit status. Use Node with
built-in fetch and WebSocket. No content is distributed with the corpus.

The immutable manifest freezes all 17 map queries, valid player classes,
Classic/Balanced/High settings at requested 2x MSAA, four walking/turning capture
points, and four history frames per variant. Additional cases cover Ultra water
on the ocean/mirror maps, bilinear/spatial/fast FXAA, and deliberate context loss.
`--cases id,id` is an iteration smoke, not the complete gate. The summary records missing required variants and
whether the whole manifest ran and its SHA-256. Driver-supported sample counts
are reported rather than silently assumed.

Each point captures three, the previous Rust compiled-source bridge, the owned
sources, and three again. All variants see the same tick and temporal jitter
sequence, reset history once at the start of their sequence, and redraw static
shadow caches. Far and resolved scene stages and the presented image including
the weapon and HUD are checked. GPU deaths, context loss, blank frames, disabled
Rust, absent draws, mismatched ticks and GL errors invalidate a point. The final
context-loss case separately verifies restored three fallback.

Each post step is also replayed on identical inputs with the bridge and owned
program, and the HUD is replayed over the identical pre-HUD canvas. Half-float
targets are compared as half-float bits. The requested variant runs last so
complete-frame comparisons keep their original meaning. The per-step gate is
exact; complete-frame owned-versus-bridge differences are also reported. Three-versus-three and both three-versus-
Rust comparisons are retained separately. In particular smoke/flames on geo1,
steam on bes2, rain/lightning on kam1 and dry ice on rhn1 can change between
renders without a simulation tick. A noisy control is evidence to inspect, not
a blanket tolerance. High yav2 already has a pre-existing three-versus-Rust
residual; the baseline verdict remains separate and must not be called exact.
The corpus does not claim to visit every possible effect or camera position.

# Compilation and caches

Owned source removes the dependency on three's first draw and makes variants
known before drawing. It enables prewarming, pacing work behind the loading
screen, source/layout content-addressed caches, and eventually avoiding three's
duplicate compile for covered materials. Tagged post and HUD passes can draw before three has compiled their materials,
avoiding that duplicate compilation on the covered path. Three compiles them
on demand when fallback is exercised. This packet keeps the preparation frames
and initial three compilation required by unported families.
Consequently it does not promise to remove the existing long loading task.

The current cache is bounded by the fixed variants used in one context. It
avoids repeated linking of that variant across materials, not driver work for a
new context. A persistent source cache would include shader/schema version,
variant flags, content hashes, backend and driver/GPU identity. WebGL offers no
portable program-binary persistence; Chromium/ANGLE's disk shader cache is
opportunistic. Native program binaries require fingerprint validation and a
source fallback. Source ownership alone cannot remove ANGLE translation or the
first driver's compile. Timings must separate generation, compile/link and
status/reflection waits, total ready time and worst main-thread task; farm
correctness timings are not slow-PC performance measurements.

`tools/renderer-corpus/load.mjs` uses the same loader and case settings for
local load diagnostics. Set `BFE_SHADER_BRIDGE=1` for the old compiled-source
path. It records GL compile/link/status/reflection and initial draw API calls,
source submissions versus unique source texts, long tasks, preparation and ready
times, and per-program registration durations. `BFE_SHADER_CACHE_PROBE=1` additionally
registers all declared variants and repeats each registration after loading,
recording context-local ID reuse separately from startup work. Use fresh applications in
interleaved rounds; record whether driver caches were retained. It deliberately
does not run through the farm performance gate.

The planar mirror case aims from above four water cells and requires Rust mirror
phase draws and reflection-stage captures; the ocean cases use the ocean's own
reflection path. Each point records differences outside its control's per-pixel
amplitude envelope, including differences on pixels where the control was stable.
Control counts are not used as a blanket pixel budget. Shader pairing compares
RGB channels, matching the existing presented-frame parity criterion.
