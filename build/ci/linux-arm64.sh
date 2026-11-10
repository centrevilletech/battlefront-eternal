#!/usr/bin/env bash
# Native Linux ARM64 (ubuntu-24.04-arm) CI job: run the self-contained
# cross-architecture published-bytes determinism gate and all workspace native
# crate tests with --no-fail-fast and RUST_BACKTRACE=1 --nocapture so any
# architecture-specific golden divergence prints its complete assertion output
# in the workflow log.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

manifest="rust/Cargo.toml"

# The ARM64 toolchain links with GNU ld, which holds every test binary's debug information in memory
# while several link at once: the hosted runner ran out of memory and was shut down mid-build once
# the client grew. Line tables keep file and line in a panic's backtrace at a fraction of the size,
# and three jobs leave room for the linkers.
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only
export CARGO_BUILD_JOBS=3

echo "==> Running cross-architecture published-bytes determinism gate on $(uname -m)"
RUST_BACKTRACE=1 cargo test --manifest-path "${manifest}" --locked \
  -p sim --test arm64_determinism \
  -p bfe-presentation --test unit_oracle \
  -- --nocapture

base_test_crates=(
  sim
  sim-replay
  bfe-render
  bfe-presentation
  bfe-pose
  content
  v8math
  jsrt
  three
  audio
  audio-decode
  basis-transcode
  texture-codec
  image-codec
  bfe-environment
  audio-control
  bfe-client
  bfe-content-native
  bfe-transport
  bfe-online
)

wave_crates=(
  bfe-ui
  bfe-interface
  bfe-shell
  bfe-video
  bfe-battle-ui
  bfe-desktop
  bfe-server
  xtask
)

test_pkg_args=(--features sim/replay)
for crate in "${base_test_crates[@]}"; do
  test_pkg_args+=(-p "${crate}")
done
for crate in "${wave_crates[@]}"; do
  if [[ -f "rust/${crate}/Cargo.toml" ]]; then
    test_pkg_args+=(-p "${crate}")
  fi
done

echo "==> Running workspace native crate tests on $(uname -m) (--no-fail-fast, RUST_BACKTRACE=1, --nocapture)"
RUST_BACKTRACE=1 cargo test --manifest-path "${manifest}" --locked --no-fail-fast \
  "${test_pkg_args[@]}" -- --nocapture
