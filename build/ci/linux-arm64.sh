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
  bfe-native
  bfe-transport
  bfe-online
)

wave_crates=(
  bfe-interface
  bfe-shell
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
