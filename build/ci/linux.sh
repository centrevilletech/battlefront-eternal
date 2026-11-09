#!/usr/bin/env bash
# Native Linux CI gate: formatting check, per-crate clippy (-D warnings),
# workspace crate test suite, ALSA null-device audio smoke, and CLI/server
# startup smoke.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

manifest="rust/Cargo.toml"

# Flip these to 1 after the workspace hygiene pass formats and clears
# lint warnings across the pre-existing crates.
BFE_CI_STRICT_FMT="${BFE_CI_STRICT_FMT:-0}"
BFE_CI_STRICT_CLIPPY="${BFE_CI_STRICT_CLIPPY:-0}"

echo "==> Step 1: cargo fmt --check"
if [[ "${BFE_CI_STRICT_FMT}" == "1" ]]; then
  cargo fmt --manifest-path "${manifest}" --all -- --check
else
  if cargo fmt --manifest-path "${manifest}" --all -- --check >/dev/null 2>&1; then
    echo "cargo fmt --check: clean"
  else
    echo "::warning::cargo fmt --check reported formatting drift in pre-hygiene crates (set BFE_CI_STRICT_FMT=1 in build/ci/linux.sh to enforce)"
  fi
fi

# Crates that already pass clippy --all-targets --no-deps -- -D warnings cleanly
# on the baseline branch.
strict_clippy_crates=(
  sim-replay
  basis-transcode
  texture-codec
  bfe-online
)

# New crates introduced across the native port lanes. Each is added to both the
# strict clippy gate and the test matrix as soon as its Cargo.toml lands.
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

present_wave_crates=()
for crate in "${wave_crates[@]}"; do
  if [[ -f "rust/${crate}/Cargo.toml" ]]; then
    present_wave_crates+=("${crate}")
    strict_clippy_crates+=("${crate}")
  fi
done

pre_hygiene_crates=(
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
  image-codec
  bfe-environment
  audio-control
  bfe-client
  bfe-content-native
  bfe-transport
)

echo "==> Step 2: cargo clippy (-D warnings) on strict crate list: ${strict_clippy_crates[*]}"
clippy_pkg_args=()
for crate in "${strict_clippy_crates[@]}"; do
  clippy_pkg_args+=(-p "${crate}")
done
cargo clippy --manifest-path "${manifest}" --locked --all-targets --no-deps \
  "${clippy_pkg_args[@]}" -- -D warnings

if [[ "${BFE_CI_STRICT_CLIPPY}" == "1" ]]; then
  echo "==> Step 2b: cargo clippy (-D warnings) on remaining workspace crates"
  extra_clippy_args=(--features sim/replay)
  for crate in "${pre_hygiene_crates[@]}"; do
    extra_clippy_args+=(-p "${crate}")
  done
  cargo clippy --manifest-path "${manifest}" --locked --all-targets --no-deps \
    "${extra_clippy_args[@]}" -- -D warnings
else
  echo "Note: skipping blocking clippy on pre-hygiene crates (${pre_hygiene_crates[*]}); set BFE_CI_STRICT_CLIPPY=1 after hygiene pass."
fi

echo "==> Step 3: cargo test across native workspace crates"
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

test_pkg_args=(--features sim/replay)
for crate in "${base_test_crates[@]}" "${present_wave_crates[@]}"; do
  test_pkg_args+=(-p "${crate}")
done

cargo test --manifest-path "${manifest}" --locked "${test_pkg_args[@]}"

echo "==> Step 4: Native audio null-device test (ALSA null plugin)"
alsa_conf="$(mktemp "${TMPDIR:-/tmp}/bfe-alsa-null.XXXXXX")"
smoke_tmp="$(mktemp -d "${TMPDIR:-/tmp}/bfe-ci-smoke.XXXXXX")"
trap 'rm -rf "${alsa_conf}" "${smoke_tmp}"' EXIT
cat > "${alsa_conf}" <<'EOF'
</usr/share/alsa/alsa.conf>
pcm.!default {
  type plug
  slave {
    pcm {
      type null
    }
    rate 48000
    channels 2
    format S16_LE
  }
}
EOF

ALSA_CONFIG_PATH="${alsa_conf}" \
BFE_AUDIO_NULL=1 \
  cargo test --manifest-path "${manifest}" --locked -p bfe-client \
    --test audio -- --ignored --nocapture

echo "==> Step 5: Native online-tool and dedicated server startup smoke"
cargo build --manifest-path "${manifest}" --locked -p bfe-client --bin online-tool
online_tool="rust/target/debug/online-tool"
"${online_tool}" --help >/dev/null

identity_out="$("${online_tool}" --identity "${smoke_tmp}/identity.key" identity show)"
if [[ "${identity_out}" != *"Fingerprint:"* ]]; then
  echo "error: unexpected online-tool identity output: ${identity_out}" >&2
  exit 1
fi

if [[ -f "rust/bfe-server/Cargo.toml" ]]; then
  echo "==> Testing bfeserver CLI and --serve operator startup"
  cargo build --manifest-path "${manifest}" --locked -p bfe-server --bin bfeserver
  bfeserver_bin="rust/target/debug/bfeserver"
  version_out="$("${bfeserver_bin}" --version)"
  echo "bfeserver version: ${version_out}"
  if [[ "${version_out}" != bfeserver* ]]; then
    echo "error: unexpected bfeserver --version output: ${version_out}" >&2
    exit 1
  fi

  server_state="${smoke_tmp}/server-state"
  mkdir -p "${server_state}"
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
fi

echo "==> Native Linux CI checks passed"
