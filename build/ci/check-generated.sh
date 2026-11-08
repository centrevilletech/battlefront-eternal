#!/usr/bin/env bash
# Verify that generated Rust layout, shader, and bridge sources match their
# generators without uncommitted drift.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

echo "==> Checking native render and presentation bridge tables"
python3 tools/native-render-bridge.py --check
python3 tools/native-presentation-bridge.py --check

if [[ -f "rust/xtask/Cargo.toml" ]]; then
  echo "==> check-generated path: rust/xtask/Cargo.toml present; running cargo xtask check"
  (cd rust && cargo xtask check)
else
  # TODO(lead): Delete this npm fallback once the xtask generators crate lands.
  echo "==> check-generated path: rust/xtask/Cargo.toml not present; running npm ci && npm run rust:check"
  npm ci
  npm run rust:check
fi
