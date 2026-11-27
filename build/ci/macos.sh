#!/usr/bin/env bash
# Native macOS CI gate (GitHub-hosted Apple silicon runner), mirroring
# build/ci/linux.sh: formatting check, per-crate clippy (-D warnings), the
# workspace crate test suite, the dedicated server startup smoke, and the
# Python tests of the preparation and packaging tools. The Linux script's
# ALSA null-device audio test has no macOS counterpart: the hosted runner has
# no sound output to open.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

manifest="rust/Cargo.toml"

# The hosted Apple silicon runner has 7 GB of memory and three cores. Line
# tables keep file and line in a panic's backtrace at a fraction of the size
# of full debug information.
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}"
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG:-line-tables-only}"

echo "==> Step 1: cargo fmt --check"
if cargo fmt --manifest-path "${manifest}" --all -- --check >/dev/null 2>&1; then
  echo "cargo fmt --check: clean"
else
  echo "::warning::cargo fmt --check reported formatting drift in pre-hygiene crates (build/ci/linux.sh decides when this becomes an error)"
fi

# The same lists as build/ci/linux.sh.
strict_clippy_crates=(
  basis-transcode
  bfe-online
  sim-gen
  bfe-script
  bfe-ui
  bfe-interface
  bfe-shell
  bfe-video
  bfe-battle-ui
  bfe-desktop
  bfe-server
  bfe-family
  bfe-storage
  xtask
)
test_crates=(
  sim
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
  bfe-environment
  audio-control
  bfe-client
  bfe-content-native
  bfe-transport
  bfe-online
  sim-gen
  bfe-script
  bfe-ui
  bfe-interface
  bfe-shell
  bfe-video
  bfe-battle-ui
  bfe-desktop
  bfe-server
  bfe-family
  bfe-storage
  xtask
)

echo "==> Step 2: cargo clippy (-D warnings) on strict crate list: ${strict_clippy_crates[*]}"
clippy_pkg_args=()
for crate in "${strict_clippy_crates[@]}"; do
  clippy_pkg_args+=(-p "${crate}")
done
cargo clippy --manifest-path "${manifest}" --locked --all-targets --no-deps \
  "${clippy_pkg_args[@]}" -- -D warnings

echo "==> Step 3: cargo test across native workspace crates"
test_pkg_args=()
for crate in "${test_crates[@]}"; do
  test_pkg_args+=(-p "${crate}")
done
cargo test --manifest-path "${manifest}" --locked "${test_pkg_args[@]}"

# The script runtime must give the same bits with the optimizer on: its tests run in
# release as well (exceptional arithmetic, the differential corpus, hostile bounds).
if [[ -f "rust/bfe-script/Cargo.toml" ]]; then
  echo "==> Step 3b: bfe-script tests in release"
  cargo test --manifest-path "${manifest}" --locked --release -p bfe-script
fi

echo "==> Step 4: Dedicated server startup smoke"
smoke_tmp="$(mktemp -d "${TMPDIR:-/tmp}/bfe-ci-smoke.XXXXXX")"
trap 'rm -rf "${smoke_tmp}"' EXIT
cargo build --manifest-path "${manifest}" --locked -p bfe-server --bin bfeserver
bfeserver_bin="${CARGO_TARGET_DIR:-rust/target}/debug/bfeserver"
server_state="${smoke_tmp}/server-state"
mkdir -p "${server_state}"
version_out="$(BFE_STATE_DIR="${server_state}" "${bfeserver_bin}" --version)"
echo "bfeserver version: ${version_out}"
if [[ "${version_out}" != bfeserver* ]]; then
  echo "error: unexpected bfeserver --version output: ${version_out}" >&2
  exit 1
fi
BFE_STATE_DIR="${server_state}" PORT=28450 "${bfeserver_bin}" --serve >"${smoke_tmp}/bfeserver.log" 2>&1 &
srv_pid=$!
status_ok=0
for _ in $(seq 1 30); do
  if status_json="$(BFE_STATE_DIR="${server_state}" PORT=28450 "${bfeserver_bin}" --admin '{"action":"status"}' 2>/dev/null)"; then
    if [[ "${status_json}" == *"\"ok\": true"* ]]; then
      status_ok=1
      break
    fi
  fi
  sleep 0.1
done
kill "${srv_pid}" 2>/dev/null || true
wait "${srv_pid}" 2>/dev/null || true
if [[ "${status_ok}" -ne 1 ]]; then
  echo "error: bfeserver --serve did not respond to operator status query" >&2
  cat "${smoke_tmp}/bfeserver.log" >&2 || true
  exit 1
fi

echo "==> Step 5: Python tests of the preparation and packaging tools"
# BFE_CI_PYTHON: a Python that has the tools' packages (the fetched toolchain's own).
"${BFE_CI_PYTHON:-python3}" -B build/ci/python-tests.py

echo "==> Native macOS CI checks passed"
