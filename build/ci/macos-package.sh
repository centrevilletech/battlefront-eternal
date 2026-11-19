#!/usr/bin/env bash
# macos-package.sh PLATFORM VERSION [PROFILE]: build the client and the server
# for one Mac architecture, stage them, make the app bundle, the disk image and
# the update payload, and check all of them (build/native/check-packages.py).
#
#   PLATFORM  darwin-arm64 or darwin-x64
#   VERSION   the release version (0.1.0-preview.N)
#   PROFILE   release (default: the workspace's release profile, with LTO), or
#             ci (the same optimisation without LTO, for runners whose memory
#             the LTO link does not fit; never for a build that is shipped)
#
# The app is signed ad hoc unless BFE_MAC_SIGN_IDENTITY names a certificate
# (build/native/sign-macos.py). Output in release/. With BFE_MAC_BUILD_INPUT=1
# the checked app is also packed as macos-<arch>-unsigned-build-input.tar.zst,
# the one file a release carries for the signing step that makes the download.
set -euo pipefail

platform="${1:?darwin-arm64 or darwin-x64}"
version="${2:?release version}"
profile="${3:-release}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${repo_root}"

case "${platform}" in
  darwin-arm64) target=aarch64-apple-darwin; arch=arm64 ;;
  darwin-x64) target=x86_64-apple-darwin; arch=x64 ;;
  *) echo "error: unknown platform ${platform}" >&2; exit 2 ;;
esac
if [[ "${profile}" == ci ]]; then
  export CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
elif [[ "${profile}" != release ]]; then
  echo "error: unknown profile ${profile}" >&2; exit 2
fi
command -v zstd >/dev/null || brew install zstd

echo "==> Building bfe-client and bfeserver for ${target} (${profile})"
rustup target add "${target}"
# /usr/bin/time -l reports the build's peak memory, which decides whether a runner can make it.
/usr/bin/time -l cargo build --manifest-path rust/Cargo.toml --release --locked --target "${target}" --bin bfe-client --bin bfeserver
bin_dir="${CARGO_TARGET_DIR:-rust/target}/${target}/release"

echo "==> Content preparation toolchain (mac-${arch})"
[[ -x "toolchain/mac-${arch}/python/bin/python3" ]] || python3 build/native/fetch-toolchain.py mac "${arch}"

echo "==> Staging, app bundle, disk image and update payload"
curl -fsSL https://get.battlefronteternal.com/downloads/updates/blobs.json -o build/known-blobs.json || echo '{"blobs":{}}' > build/known-blobs.json
release_json=()
[[ -f build/bfe-release.json ]] && release_json=(--release-json build/bfe-release.json)
python3 build/native/stage.py \
  --platform "${platform}" \
  --client-bin "${bin_dir}/bfe-client" \
  --server-bin "${bin_dir}/bfeserver" \
  --toolchain-dir "toolchain/mac-${arch}" \
  --out-dir "release/mac-${arch}-unpacked" \
  ${release_json[@]+"${release_json[@]}"}
python3 build/native/package-macos.py \
  --staged-dir "release/mac-${arch}-unpacked" \
  --platform "${platform}" \
  --version "${version}" \
  --out-dir release
python3 build/native/update-payload.py \
  --tree "release/macos-${arch}" \
  --platform "${platform}" \
  --version "${version}" \
  --known build/known-blobs.json \
  --out "release/update-${platform}.tar"

echo "==> Checking the packages"
python3 build/native/check-packages.py \
  --platform "${platform}" \
  --staged-dir "release/mac-${arch}-unpacked" \
  --macos-app "release/macos-${arch}/Battlefront Eternal.app" \
  --dmg "release/Battlefront-Eternal-${version}-macos-${arch}.dmg" \
  --update-payload "release/update-${platform}.tar" \
  --report-out "release/size-report-${platform}.md"

if [[ "${BFE_MAC_BUILD_INPUT:-}" == 1 ]]; then
  echo "==> Packing the build input for the signing step"
  python3 build/native/package-macos.py \
    --app "release/macos-${arch}/Battlefront Eternal.app" \
    --platform "${platform}" \
    --version "${version}" \
    --out-dir release \
    --skip-dmg --build-input
fi
