#!/usr/bin/env bash
# Build the client binaries/tests first. Synthetic facts and PCM exercise the
# production WASM host, native control output and complete native DSP chain.
set -euo pipefail
archive=$(mktemp -d)
trap 'rm -rf "$archive"' EXIT
bin=${BFE_CLIENT_BIN:-rust/target/debug}
for quality in original enhanced; do
 node tools/native-sound/capture.mjs "$archive" "control-$quality-stereo"
 "$bin/native-sound" "$archive"
 BFE_SOUND_FIXTURE="$archive" cargo test --locked --manifest-path rust/Cargo.toml -p bfe-client --test sound -- --ignored
 done
BFE_SOUND_BURST=1 node tools/native-sound/capture.mjs "$archive" control-enhanced-stereo
"$bin/native-sound" "$archive"
BFE_SOUND_FIXTURE="$archive" cargo test --locked --manifest-path rust/Cargo.toml -p bfe-client --test sound -- --ignored
