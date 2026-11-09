#!/usr/bin/env bash
# Verify that generated Rust layout and shader sources match their generators
# without uncommitted drift.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

echo "==> Checking generated Rust schemas, layouts, and shader tables"
(cd rust && cargo xtask check)
