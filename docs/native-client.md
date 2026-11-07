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
next baseline. Recorded bulk transfers and actor matrix/skin bindings consume
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
