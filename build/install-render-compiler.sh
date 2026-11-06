#!/bin/bash
# A dated, signed Ubuntu archive keeps C++ WebAssembly reproducible after the
# live repository replaces LLVM. Other repositories cannot affect this install.
set -euo pipefail
snapshot=20261105T000000Z
version=1:20.1.2-0ubuntu1~24.04.3
compiler_dir=$(mktemp -d)
trap 'sudo rm -rf "$compiler_dir"' EXIT
# Keep Ubuntu signature verification enabled with the system archive keyring.
chmod 755 "$compiler_dir"
mkdir -p "$compiler_dir/lists/partial"
chmod 755 "$compiler_dir/lists" "$compiler_dir/lists/partial"
sudo chown _apt:root "$compiler_dir/lists/partial"
cat > "$compiler_dir/ubuntu.list" <<EOF
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://snapshot.ubuntu.com/ubuntu/$snapshot/ noble main universe
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://snapshot.ubuntu.com/ubuntu/$snapshot/ noble-updates main universe
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://snapshot.ubuntu.com/ubuntu/$snapshot/ noble-security main universe
EOF
# Use private indexes rather than apt's snapshot switch: unrelated source-list
# combinations must not change either resolution or apt's snapshot handling.
options=(-o "Dir::Etc::sourcelist=$compiler_dir/ubuntu.list" -o Dir::Etc::sourceparts=- -o "Dir::State::lists=$compiler_dir/lists" -o Acquire::Check-Valid-Until=false)
# The snapshot service answers 502/503 now and then. Fetch only the package
# indexes (no translations or command-not-found data), let apt retry each
# file, and retry the whole update: a half-fetched index must not pass.
options+=(-o Acquire::Retries=8 -o Acquire::Languages=none -o Acquire::IndexTargets::deb::CNF::DefaultEnabled=false)
for attempt in 1 2 3 4 5; do
 if sudo apt-get "${options[@]}" -o APT::Update::Error-Mode=any update; then break; fi
 [ "$attempt" = 5 ] && { echo "The archive snapshot could not be read" >&2; exit 1; }
 sleep $((attempt * 20))
done
packages=()
for package in clang-20 llvm-20 lld-20 libllvm20 libclang-cpp20 libclang1-20 libclang-common-20-dev llvm-20-runtime llvm-20-linker-tools llvm-20-tools; do
 apt-cache "${options[@]}" madison "$package" | grep -F " $version " >/dev/null
 packages+=("$package=$version")
done
# The farm can validate resolution without modifying its installed compiler.
# CI uses the default install and checks the version again before generation.
extra=()
if [ "${1-}" = --simulate ]; then extra+=(--simulate); fi
sudo apt-get "${options[@]}" install -y --no-install-recommends --allow-downgrades "${extra[@]}" "${packages[@]}"
