# Synthetic audio goldens

`renders.json` records Chromium 153.0.8010.12 renders of the audio graph at
revision `fd5099e2`, before the browser DSP was removed. `audio-fixture.js`
defines deterministic noise/tones, timeline commands and layouts without
game assets. Stream fixtures use the frozen renderer's native bed/voice
entrypoints to schedule the same buffers and controls.

Non-strict renders are gzip-compressed, channel-major float32 little endian.
Strict renders retain SHA-256 of those same uncompressed bytes. Signed zero
is significant to their hashes. The complete golden gate runs with a PCM
host in CI and with Chromium OfflineAudioContext in the browser gate.

`adpcm.json` holds exact hashes from the same frozen decoder over generated
byte streams, including clipping, index clamping and partial trailing blocks.

`dsp.json` stores float32 output hashes and float64 meter/limiter readings
from the former processor parity fixtures. Its input generators remain in
`audio-dsp-parity.mjs`. `hrtf.f32` holds stereo impulse renders at 44.1/48 kHz
for the directions listed in the Rust HRTF test; the impulse is at frame 512.
These derived responses use the resource notices in THIRD-PARTY-NOTICES.md.

Goldens are recorded reference data, never regenerated from the candidate
when checking it. Every approximate comparison is below −96 dBFS per sample.

`frontend.json` records the frozen shell/loading gain topology over generated
mono/stereo tones at 44.1/48 kHz, including resampled one-shots, late loop
offsets, music dips and pulse end fades. The inputs live in
`audio-frontend-fixture.js`; the live node reference was removed.
