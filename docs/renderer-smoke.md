# Renderer smoke check

`node tools/renderer-smoke.mjs warp` runs the committed Rust renderer WASM
and three.js in Electron on Windows, requesting ANGLE D3D11 WARP. It fails
if the unmasked renderer does not identify Microsoft software D3D11. No
prepared game directory, downloads of game content, or game loader are used.
The workflow-dispatch-only `renderer-smoke` job in CI uses GitHub-hosted
Windows and Linux runners. Dispatch CI on the branch containing the check.

On Linux, run `xvfb-run -a node tools/renderer-smoke.mjs gl` for ANGLE OpenGL,
with `LIBGL_ALWAYS_SOFTWARE=1` to request Mesa software rendering. The same
command with `swiftshader` selects ANGLE SwiftShader. A Linux installation
without a configured Electron sandbox can set `BFE_SMOKE_NO_SANDBOX=1` for
this isolated test process.

Each backend runs with WEBGL_multi_draw, then with that extension hidden to
exercise the repeated-draw fallback. A successful run requires:

- Every compiled program links, every observed draw and frame is free of
  WebGL errors, and Rust remains enabled without per-item fallback errors.
- All 15 Rust pass counters report actual draws: world, instances, far,
  terrain, transparent, effects, foliage, view, post, HUD, water, shadow,
  motion, canopies and screen.
- The generated scene is visible and its alternating three/Rust frames
  match with at most two levels of difference per RGB channel. Repeated
  frames from either owner must also be stable. The motion target is
  compared separately.
- Terrain has two separated visible index runs, an unpainted layer between
  painted layers, and a deliberately prebound comparison texture on the
  unpainted layer's unit. Rust must replace it with its empty color texture
  to avoid WebGL rejecting the whole draw.

The report prints the renderer, Electron/Chromium versions, program count,
pass counters, multi-draw count and individual assertions. Pixel failures
remain failures; the report can contain passing assertions alongside them.
The water case also delegates just the near-water pass back to three as a
diagnostic, while retaining the full Rust water comparison as a gate.

This checks the programs instantiated by these fixtures. It cannot enumerate
every game material, shader define combination, effect, texture format or
rendering configuration. The scene covers bump on/off, shadows, alpha
blending, skinned and rigid instances, bloom and soft focus, but is not a
qualification of all temporal-upscaling modes or game scenes. Two engines
can also share a shader bug: matching pixels alone cannot detect an
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
