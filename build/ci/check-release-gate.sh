#!/usr/bin/env bash
# The 2005 game's availability gate stays closed in what ships. Two pieces of
# code open it, and neither is in an ordinary build: the test-only crate
# `bfe-test-gate`, which test code hands to the servers and clients it starts,
# and the developer switch for online play, which is compiled only with
# `--cfg bfe_dev_online` (rust/bfe-family/src/dev_online.rs). Each has a marker
# text that a binary containing it carries. This check fails when:
#
#  1. any workspace crate depends on `bfe-test-gate` as a normal or build
#     dependency, for any target, with every feature of every workspace crate
#     on (the unified graph `cargo build --workspace --all-features` uses);
#  2. a binary given as an argument (a client or server) contains either
#     marker, as build/native/stage.py checks every release binary it
#     packages, or cannot be read in full: only a completed search that finds
#     no marker passes;
#  3. a release binary that does contain one of the two lacks its marker: the
#     check of (2) would then prove nothing;
#  4. a release binary built without the cfg still follows the developer
#     switch in its environment.
#
# Usage: build/ci/check-release-gate.sh [BINARY...]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
manifest="${repo_root}/rust/Cargo.toml"

marker_of() {
  local found
  found="$(sed -n 's/^pub const MARKER: &str = "\(.*\)";[[:space:]]*$/\1/p' "${repo_root}/$1")"
  if [[ -z "${found}" ]]; then
    echo "error: no MARKER constant in ${repo_root}/$1" >&2
    exit 1
  fi
  printf '%s' "${found}"
}
test_marker="$(marker_of rust/bfe-test-gate/src/lib.rs)"
switch_marker="$(marker_of rust/bfe-family/src/dev_online.rs)"

# The search's own status: 0 when the marker is found, 1 when the whole file
# was read without finding it, anything else when the file could not be read.
marker_search() {
  local status=0
  LC_ALL=C grep -a -q -F -- "$1" "$2" || status=$?
  echo "${status}"
}

echo "==> No workspace crate depends on bfe-test-gate outside tests, under any features"
dependents="$(cargo tree --manifest-path "${manifest}" --locked --workspace --all-features \
  --target all -e normal,build -i bfe-test-gate --prefix none --format '{p}' | sort -u)"
echo "${dependents}"
if [[ "$(printf '%s\n' "${dependents}" | grep -c .)" -ne 1 || "${dependents}" != bfe-test-gate* ]]; then
  echo "error: a crate other than bfe-test-gate itself depends on bfe-test-gate outside its tests" >&2
  exit 1
fi

control_dir="$(mktemp -d "${TMPDIR:-/tmp}/bfe-gate-check.XXXXXX")"
trap 'rm -rf "${control_dir}"' EXIT

# Builds the release example $2 of package $1 into $3 and names its binary.
control_build() {
  cargo build --manifest-path "${manifest}" --locked --release -p "$1" --example "$2" \
    --target-dir "${control_dir}/$3" >&2
  find "${control_dir}/$3/release/examples" -maxdepth 1 -type f -name "$2" -perm -u+x | head -n 1
}

echo "==> A release binary that uses the open gate contains its marker"
control="$(control_build bfe-test-gate open target)"
if [[ -z "${control}" || "$(marker_search "${test_marker}" "${control}")" != 0 ]]; then
  echo "error: the marker was not found in a binary that uses the open gate (${control:-not built})" >&2
  exit 1
fi

echo "==> A release binary with the developer switch compiled in contains its marker"
control="$(RUSTFLAGS="${RUSTFLAGS:-} --cfg bfe_dev_online" control_build bfe-family online_gate switched)"
if [[ -z "${control}" || "$(marker_search "${switch_marker}" "${control}")" != 0 ]]; then
  echo "error: the marker was not found in a binary with the developer switch (${control:-not built})" >&2
  exit 1
fi
if [[ "$(BFE_DEV_SWBF2_ONLINE=1 "${control}")" != *' plays swbf1 swbf2' ]]; then
  echo "error: the developer switch opens nothing where it is compiled in" >&2
  exit 1
fi

echo "==> An ordinary release binary has no developer switch to follow"
control="$(control_build bfe-family online_gate target)"
if [[ -z "${control}" || "$(marker_search "${switch_marker}" "${control}")" != 1 ]]; then
  echo "error: the developer switch is in an ordinary release binary (${control:-not built})" >&2
  exit 1
fi
if [[ "$(BFE_DEV_SWBF2_ONLINE=1 "${control}")" != 'Gate("built") plays swbf1' ]]; then
  echo "error: an ordinary release binary follows the developer switch" >&2
  exit 1
fi

status=0
for binary in "$@"; do
  if [[ ! -f "${binary}" ]]; then
    echo "error: ${binary} does not exist" >&2
    status=1
    continue
  fi
  found="$(marker_search "${test_marker}" "${binary}")/$(marker_search "${switch_marker}" "${binary}")"
  case "${found}" in
    1/1)
      echo "ok: ${binary} has no open gate and no developer switch"
      ;;
    0/*)
      echo "error: ${binary} contains the test-only open gate (${test_marker})" >&2
      status=1
      ;;
    */0)
      echo "error: ${binary} contains the developer switch for online play (${switch_marker})" >&2
      status=1
      ;;
    *)
      echo "error: ${binary} could not be read in full, so it was not checked" >&2
      status=1
      ;;
  esac
done
exit "${status}"
