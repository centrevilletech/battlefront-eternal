#!/usr/bin/env bash
# Native Windows (x86_64-pc-windows-msvc) CI gate: build native client/server
# binaries, run device-independent crate tests, and verify CLI startup.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

manifest="rust/Cargo.toml"
target="${CARGO_BUILD_TARGET:-x86_64-pc-windows-msvc}"

echo "==> Step 1: Building Windows native executables (${target})"
build_pkgs=(
  -p bfe-client
  -p bfe-online
)
if [[ -f "rust/bfe-server/Cargo.toml" ]]; then
  build_pkgs+=(-p bfe-server)
fi
if [[ -f "rust/bfe-desktop/Cargo.toml" ]]; then
  build_pkgs+=(-p bfe-desktop)
fi

cargo build --manifest-path "${manifest}" --locked --target "${target}" "${build_pkgs[@]}"

echo "==> Step 2: Running device-independent Windows crate tests (${target})"
test_crates=(
  bfe-client
  bfe-content-native
  bfe-transport
  bfe-online
)

optional_crates=(
  bfe-interface
  bfe-shell
  bfe-battle-ui
  bfe-desktop
  bfe-server
  xtask
)
for crate in "${optional_crates[@]}"; do
  if [[ -f "rust/${crate}/Cargo.toml" ]]; then
    test_crates+=("${crate}")
  fi
done

test_pkg_args=()
for crate in "${test_crates[@]}"; do
  test_pkg_args+=(-p "${crate}")
done

cargo test --manifest-path "${manifest}" --locked --target "${target}" "${test_pkg_args[@]}"

echo "==> Step 3: Windows CLI startup smoke"
bin_dir="rust/target/${target}/debug"
online_tool="${bin_dir}/online-tool.exe"
if [[ ! -f "${online_tool}" ]]; then
  echo "error: expected ${online_tool} to exist after build" >&2
  exit 1
fi

"${online_tool}" --help >/dev/null

smoke_tmp="$(mktemp -d "${TMPDIR:-/tmp}/bfe-win-smoke.XXXXXX")"
trap 'rm -rf "${smoke_tmp}"' EXIT

identity_out="$("${online_tool}" --identity "${smoke_tmp}/identity.key" identity show)"
if [[ "${identity_out}" != *"Fingerprint:"* ]]; then
  echo "error: unexpected online-tool identity output on Windows: ${identity_out}" >&2
  exit 1
fi

if [[ -f "rust/bfe-server/Cargo.toml" ]]; then
  bfeserver_bin="${bin_dir}/bfeserver.exe"
  version_out="$("${bfeserver_bin}" --version)"
  echo "bfeserver version: ${version_out}"
  if [[ "${version_out}" != bfeserver* ]]; then
    echo "error: unexpected bfeserver.exe --version output: ${version_out}" >&2
    exit 1
  fi
fi

echo "==> Native Windows CI checks passed"
