#!/usr/bin/env bash
# Install the exact Rust toolchain pinned in rust/rust-toolchain.toml using
# rustup directly (no third-party GitHub Actions).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
toolchain_file="${repo_root}/rust/rust-toolchain.toml"

channel="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "${toolchain_file}" | head -n 1)"
if [[ -z "${channel}" ]]; then
  echo "error: failed to read channel from ${toolchain_file}" >&2
  exit 1
fi

components=(rustfmt clippy)
if [[ "${1:-}" == "--minimal" ]]; then
  components=()
fi

echo "==> Installing Rust toolchain ${channel} (${components[*]:-no extra components})"
cmd=(rustup toolchain install "${channel}" --profile minimal)
for comp in "${components[@]}"; do
  cmd+=(--component "${comp}")
done
"${cmd[@]}"
rustup default "${channel}"

if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  rustup target add --toolchain "${channel}" "${CARGO_BUILD_TARGET}"
fi

rustc --version
cargo --version
