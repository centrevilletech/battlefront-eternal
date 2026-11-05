`irc-composite.f32` is the stereo, 44,100 Hz IRCAM composite impulse
response resource from Chromium's
[`Composite.flac`](https://chromium.googlesource.com/chromium/src/+/main/third_party/blink/renderer/platform/audio/resources/Composite.flac).
It contains 240 responses of 256 frames, interleaved left/right float32,
little endian. It is decoded without changing its sample rate. Signed PCM normalization
matches the host decoder: negative samples multiply by float32 `1/32768` and nonnegative
samples by float32 `1/32767`. This distinction matters to HRTF group-delay preparation.

The Rust HRTF database removes each response's average group delay and
interpolates the responses as Chromium's HRTFKernel and HRTFElevation do.
The resource uses Chromium's BSD license; the translated HRTF code also
retains its WebKit two-clause notice; see `THIRD-PARTY-NOTICES.md`.
