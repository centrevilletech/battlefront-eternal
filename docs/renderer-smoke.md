# Renderer smoke check

## Native software-OpenGL renderer smoke (`tools/renderer-smoke-native.py`)

`python3 tools/renderer-smoke-native.py` tests the native Rust shader catalog,
content loader, scene admission adapters, and OpenGL renderer (`render-smoke`,
`bfe-client`, `static-frame`, `static-fixture`, and `fingerprint`) on Linux over
Mesa software OpenGL (`LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`)
without prepared game files. The `renderer-smoke-native` job in
`.github/workflows/ci.yml` runs this check on `ubuntu-24.04` and uploads both
`renderer-smoke-native.png` and `renderer-smoke-native.jsonl` as workflow
artifacts.

The script executes five verification stages in a temporary directory:

1. **Synthetic static geometry probe (`static-fixture`):** Uses the six
   synthetic foliage definitions (`grasspatch` and `leafpatch`) and the
   synthetic water/ocean inputs, verifying that
   `rust/bfe-content-native/examples/static-fixture` reproduces the frozen
   SHA-256 digests in `tests/fixtures/static-geometry-hashes.json`.
2. **Synthetic map admission (`fingerprint`):** Builds a self-contained
   synthetic `maps/room/` content directory containing procedural checker PNG
   textures, a near lit world quad, a sky backdrop layer, a 1-bone placed prop,
   the shared `grasspatch` and `leafpatch` foliage classes, a subdivided water
   patch, and minimal `common/` and `sides/rep/` definitions, then verifies
   content manifest hashing with `rust/bfe-content-native/examples/fingerprint`.
3. **Full shader catalog compile/link and 6-family draws (`render-smoke`):**
   Cross-checks `rust/bfe-render/shaders/*/manifest.json` across all 6 families
   (`world`: 536, `actors`: 310, `effects`: 149, `interface`: 15, `post-hud`: 12,
   `utility`: 24; total 1,046) and all sub-families, then runs `render-smoke`
   over surfaceless EGL software OpenGL (`llvmpipe`). `render-smoke` compiles and
   links all 1,046 variants uncached (`cached: false, shaders: 2, links: 1`)
   through `bfe-render`'s own program builder (`Bridge::add_owned_program`),
   emits `variant-start`, `variant-result`, `family-result`, `family-draw`, and
   `runs` JSONL records, and draws one representative scene per family (`world`,
   `actors`, `effects`, `interface`, `post-hud`, `utility`) into an offscreen
   RGBA8 + depth/stencil framebuffer, verifying non-clear pixels, unique
   colors >= 2, zero GL errors, and zero renderer faults.
4. **Surfaceless EGL component renders (`static-frame`):** Runs `static-frame`
   over EGL surfaceless `llvmpipe` for `sky`, `props`, `foliage`, and `water`,
   checking that each component admits its static world and records non-zero
   draws and non-clear pixels. The four 320x180 RGBA framebuffers are combined
   into a 640x360 2x2 contact sheet PNG (`renderer-smoke-native.png`), and the
   script verifies the PNG signature, `IHDR` dimensions, color type, and
   non-blank pixel coverage on disk.
5. **Windowed X11 client frame loop (`bfe-client --map room`):** Launches
   `bfe-client --map room` under `xvfb-run -a` via
   `tools/native-client-window.py`, exercising X11 window creation, GLES 3.0
   context initialization, synthetic input delivery (`KeyW`, mouse motion,
   focus transitions), and multiple `Renderer::battle_frame` redraws.

## Shader catalog and pass coverage

At startup the renderer smoke check reads every
`rust/bfe-render/shaders/*/manifest.json` and registers every declared shader
variant through `bfe-render` before drawing the synthetic fixtures. No family or
variant list is hardcoded. Reports include each variant's compile/link result,
shader/link call counts, cache status and elapsed milliseconds, plus compile
milliseconds per family. Rejected variants are reported by key.

A successful run requires:

- Every manifest variant compiles and links. Every declared post/HUD program
  also draws; all other programs instantiated by the fixture link. Every observed
  draw and frame is free of OpenGL errors, and `bfe-render` reports no errors.
- All 15 Rust pass counters report actual draws: world, instances, far,
  terrain, transparent, effects, foliage, view, post, HUD, water, shadow,
  motion, canopies and screen. Both FXAA variants and bilinear, spatial and
  temporal post passes execute. A repeated temporal sequence is pixel-exact.
- The generated scene is visible, repeated Rust frames are stable, and the
  unlit HUD texture matches its known generated RGB colour. The motion target
  contains nonzero velocity and is stable across repeated Rust draws.
- Forced context loss and restoration rebuild the Rust resources and reproduce
  the generated frame.
- Terrain has two separated visible index runs, an unpainted layer between
  painted layers, and a deliberately prebound comparison texture on the
  unpainted layer's unit. Rust must replace it with its empty color texture
  to avoid rejecting the whole draw.
- A four-sample phase draws alpha-tested cutouts with alpha to coverage enabled.
  Terrain receivers must change pixels when shadow strength changes, both before
  and during that multisampled phase.

The catalog check covers every declared Rust-owned source variant, including
families added later. Generated draws exercise a smaller set of material and
phase contracts; they do not enumerate every loaded scene, combination of
effects, texture format or rendering configuration.

## Local HLSL translation check

ANGLE includes an independent GLSL ES translator. Its
[development guide](https://chromium.googlesource.com/angle/angle/+/HEAD/doc/DevSetup.md)
documents the `angle_shader_translator` target; the
[sample source](https://chromium.googlesource.com/angle/angle/+/HEAD/samples/shader_translator/shader_translator.cpp)
accepts `-s=w2 -b=h` for GLSL ES 3.00 to HLSL 4.1. Building that target with HLSL
support on Linux can check translation of locally collected shader sources
without sending them to CI. Collect the expanded sources actually passed
to OpenGL ES, including Rust's rewritten shaders.

## Why Windows native GL stays device-independent in CI

On `windows-latest`, the `windows` job in `.github/workflows/ci.yml`
builds `bfe-client`, `bfeserver`, `bfe-online`, and `bfe-desktop`, runs all
device-independent unit and integration tests across `x86_64-pc-windows-msvc`,
and checks CLI startup, while leaving live OpenGL context creation to the Linux
`renderer-smoke-native` job:

- **`static-frame` uses `EGL_EXT_device_enumeration`:** Headless EGL calls
  `glutin::api::egl::device::Device::query_devices()` followed by
  `Display::from_device(&device, None)` to create a surfaceless EGL context
  without an OS window. That extension is provided by Mesa (`llvmpipe`) and
  Linux GPU drivers, not by the headless Windows Server runner image.
- **Windowed `bfe-client` uses `glutin-winit`:** Creating a WGL/EGL window on
  `windows-latest` requires a hardware/WARP display adapter with OpenGL ES 3.0 /
  WGL context creation, whereas `ubuntu-24.04` provides `xvfb-run` +
  `libegl1-mesa-dev` + `libgl1-mesa-dri` (`llvmpipe`) deterministically.
- **What would be needed to enable live GL on Windows CI:** Bundling ANGLE's
  `libEGL.dll` + `libGLESv2.dll` (backed by D3D11 WARP, `D3D_DRIVER_TYPE_WARP`)
  alongside the test runner and setting the window creation preference in
  `rust/bfe-client` (`session_window.rs`) to `PreferEgl` with ANGLE's
  surfaceless/offscreen pbuffer extension on Windows.
