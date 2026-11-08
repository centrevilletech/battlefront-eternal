# Renderer smoke check

`node tools/renderer-smoke.mjs warp` runs the committed Rust renderer WASM
in Electron on Windows, requesting ANGLE D3D11 WARP. It fails
if the unmasked renderer does not identify Microsoft software D3D11. No
prepared game directory, downloads of game content, or game loader are used.
The workflow-dispatch-only `renderer-smoke` job in CI uses GitHub-hosted
Windows and Linux runners. Dispatch CI on the branch containing the check.

On Linux, run `xvfb-run -a node tools/renderer-smoke.mjs gl` for ANGLE OpenGL,
with `LIBGL_ALWAYS_SOFTWARE=1` to request Mesa software rendering. The same
command with `swiftshader` selects ANGLE SwiftShader. A Linux installation
without a configured Electron sandbox can set `BFE_SMOKE_NO_SANDBOX=1` for
this isolated test process.

At startup the launcher reads every `rust/bfe-render/shaders/*/manifest.json`.
It passes the discovered keys through a temporary catalog to Electron, which
registers every variant through Rust before drawing the fixtures. No family or
variant list is hardcoded. Reports include each variant's compile/link result,
shader/link call counts, cache status and elapsed milliseconds, plus compile
milliseconds per family. Rejected variants are reported by key. CI retains the
JSON-lines report as `renderer-smoke-<backend>`; local runs can save it with
`BFE_SMOKE_OUTPUT`. These are registration/driver-wait timings; a new context
can still reuse the driver's disk shader cache.

Each backend runs with WEBGL_multi_draw, then with that extension hidden to
exercise repeated Rust draws. The driver process has a twelve-minute limit
within the CI job's fifteen-minute limit. A successful run requires:

- Every manifest variant compiles and links. Every declared post/HUD program
  also draws; all other programs instantiated by the fixture link. Every observed draw and frame is free of
  WebGL errors, and the required Rust renderer reports no errors.
- All 15 Rust pass counters report actual draws: world, instances, far,
  terrain, transparent, effects, foliage, view, post, HUD, water, shadow,
  motion, canopies and screen. Both FXAA variants and bilinear, spatial and
  temporal post passes execute. A repeated temporal sequence is pixel-exact.
- The generated scene is visible, repeated Rust frames are stable, and the
  unlit HUD texture matches its known generated RGB colour. The motion target
  contains nonzero velocity and is stable across repeated Rust draws.
- Forced context loss and restoration rebuild the Rust resources and reproduce
  the generated frame. The battle draw guard records no three.js calls.
- Terrain has two separated visible index runs, an unpainted layer between
  painted layers, and a deliberately prebound comparison texture on the
  unpainted layer's unit. Rust must replace it with its empty color texture
  to avoid WebGL rejecting the whole draw.
- A four-sample phase draws alpha-tested cutouts with alpha to coverage enabled.
  Terrain receivers must change pixels when shadow strength changes, both before
  and during that multisampled phase.

The report prints the renderer, Electron/Chromium versions, program count,
pass counters, multi-draw count and individual assertions. Pixel failures
remain failures; the report can contain passing assertions alongside them.

The catalog check covers every declared Rust-owned source variant, including
families added later. Generated draws exercise a smaller set of material and
phase contracts; they do not enumerate every loaded scene, combination of
effects, texture format or rendering configuration. The scene covers bump on/off, shadows, alpha
blending, skinned and rigid instances, bloom and soft focus, including settings changes, but does not qualify all game scenes. A renderer and its fixtures
can share a shader bug: stable pixels alone cannot detect an
unwanted texture fetch hoisted by a compiler. WARP exercises the Windows
shader translation/compiler/runtime path, not physical GPU drivers, VRAM
pressure, device removal, or GPU performance.

## A local HLSL translation check

ANGLE includes an independent GLSL ES translator. Its
[development guide](https://chromium.googlesource.com/angle/angle/+/HEAD/doc/DevSetup.md)
documents the `angle_shader_translator` target; the
[sample source](https://chromium.googlesource.com/angle/angle/+/HEAD/samples/shader_translator/shader_translator.cpp)
accepts `-s=w2 -b=h` for WebGL2 to HLSL 4.1. Building that target with HLSL
support on Linux can check translation of locally collected shader sources
without sending them to CI. Collect the expanded sources actually passed
to WebGL, including Rust's rewritten shaders, rather than GLSL templates
with unresolved three.js includes.

The sample's built-in resource limits are deliberately small, including
eight combined texture units. A game-shader check needs resource limits and
extensions matching the WebGL context; an unmodified sample can reject
otherwise valid shaders. HLSL output alone does not run Microsoft's HLSL
compiler, link programs or validate sampler bindings at draw time. The
Windows WARP job covers those later stages for the synthetic fixtures.
