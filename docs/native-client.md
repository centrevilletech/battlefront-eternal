# Native battle client

`bfe-client` opens a winit window, owns its GL context, maps desktop input to
generated command records, and feeds the Rust audio master through cpal. A full
recorded session runs the existing Rust presentation and renderer phases. It
needs prepared game content; neither game files nor recordings belong in the
repository.

Build with `cargo build --manifest-path rust/Cargo.toml -p bfe-client` and run
`bfe-client SESSION_DIR CONTENT_DIR`. `BFE_CONTENT_DIR` can supply the content
root. `--pcm STEREO_F32LE` supplies a looping 48 kHz stereo test signal. The
`native-session` diagnostic uses the same window loop without opening an audio
device and prints a framebuffer comparison before its last swap. Battle output
lives in a fixed RGBA8 canvas target, then blits to the window. An overlapping
window cannot discard replay fragments through X11 pixel ownership.

`bfe-content-native::Loader` reads the prepared maps, sides and common files.
It uses the shared native definition, clip, model, collision and BVH loaders,
then constructs visible geometry, material descriptors, texture resources and
unit/first-person presentation content. PNG/lossless WebP and KTX2 use the same
Rust decoders and device format policy as the browser. A declared compressed
twin must load successfully. Paths remain inside the content directory.

`World` receives generated commands at fixed authority steps and lends its
presentation frame to the host. Its associated frame type permits a live native
owner to supply retained phase inputs directly. `RecordedWorld` instead lends
ranges of admitted session operations, preserving the recorded timestamps and
ordering. Device clocks do not replace those timestamps.

The recording tool is `tools/native-session/capture.mjs`, used with the existing
renderer-corpus CDP battle loader. Run `session-schema` once and name its JSON
output with `NATIVE_SCHEMA`. `CASE` selects a corpus case. For an Electron
reference, `BFE_SETTINGS_FILE` names its active profile settings file. The
recorder checks the applied graphics values and stores them in the archive. The tool instruments
WASM exports and host GL state outside the application sources, records the
snapshot timeline and presentation inputs, and rejects unhandled render calls.
It records only host writes between phase calls; Rust output banks become the
next baseline. Recorded collision answers feed the vehicle owner between placement and pose.
Recorded bulk transfers and actor matrix/skin bindings consume
native presentation outputs during playback. The window submits world, far,
terrain, foliage, water, effects, actors, postprocessing and HUD through their
existing Rust passes.

The archive stores a versioned API digest, stable owner ids, typed blob spans,
phase calls, changed input ranges, output digests and a separate reference image.
Matching geometry and image arguments resolve to the native loader's assets by
content digest. Native unit and first-person descriptors must match registration
before admission. Reference pixels are used only by the final comparison and
never become a render texture or presentation input.

`BFE_SESSION_VALIDATE=1` checks recorded renderer submissions;
`BFE_SESSION_GL_CHECK=1` checks GL errors. Both are diagnostic work outside normal
frame submission. The comparison prints both framebuffer hashes, changed pixels
and channels, maximum error, RMS error, first/largest differences, spatial counts
and presentation-output mismatches. Repeat a differing browser reference with
`tools/native-session/control.mjs`. Remove archives after comparison and retain
only summaries.

The original near-only capture format remains supported by the headless
`bfe-native` regression tool and the client. Full sessions require `session.json`;
a failed full session never switches to the near renderer. Replay currently
carries the recorded camera and pass schedule. It does not construct an
interactive authoritative native battle or a menu/transport service.

`bfe-client --map MAP CONTENT_DIR --seconds 20` admits an inspection scene from
prepared files. The free camera uses the generated input records. Admission
includes opaque near/far world and terrain, sky layers following the camera,
initial built/unbuilt prop rigs and grounded trap pads, placed foliage, and Low water
or Classic ocean surfaces. Map lighting and fog come from prepared metadata. Prop vertex packing,
skeleton binding, foliage packing/selection and drawing use the existing Rust
owners. The frame loop retains its geometry, skeleton and input banks.

Inspection uses a deterministic cold random stream for grass and leaf tint.
Recorded comparisons can supply the browser's cold draws to the same loader.
Water retains its first mask frame and time zero; the shared surface owner
positions the Classic ocean grid from the camera. Animated doors/props,
flyer motion, world-driven effects, contact
ripples, reflections and postprocessing require further host admission. Blended
world buckets remain on the full recorded-session path. Inspection does not
invent health changes, soldier state or authority ticks.

`tools/native-static-fixture.mjs` compares foliage cards and water construction
with the browser constructors on public synthetic inputs. Build its probe with
`cargo build --manifest-path rust/Cargo.toml -p bfe-content-native --example static-fixture`.
`tools/native-static-frame.mjs` records isolated browser views and invokes the
`static-frame` binary against natively loaded content. It reports exact pixel
counts and deltas for props, foliage, water and sky. Its temporary view/image
files belong outside the repository and are removed after comparison.

Near captures are validated before renderer registration: object ids must name
earlier allocations, world bounds contain ten finite values, typed spans and
frame/bone ranges must fit their banks. Malformed files return an admission
error to both hosts. The hosted Windows renderer job also builds the native
executables for MSVC and runs their device-independent tests; its WARP renderer
check remains a separate qualification from opening a native window.
