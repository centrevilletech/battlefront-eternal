#!/usr/bin/env bash
# package-smoke-macos.sh PACKAGES OUTPUT: install a Mac build as a player would
# and run it. PACKAGES holds either a disk image (*.dmg: the published,
# notarized download) or a Release run's build input
# (macos-*-unsigned-build-input.tar.zst). The app goes to /Applications marked
# as a download (the quarantine attribute a browser sets), Gatekeeper is asked
# for its verdict, the programs and the bundled tools run, the platform smoke
# runs on the installed app, and the app is removed again. Writes checks.txt
# and the smoke output to OUTPUT.
#
# A build input carries only an ad-hoc signature, which Gatekeeper refuses by
# design: for it the verdict is recorded, and the quarantine mark is taken off
# before the app is started. A disk image must be accepted as it is.
set -uo pipefail
packages=$(cd "$1" && pwd); output=$2
mkdir -p "$output"; output=$(cd "$output" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
failed=0
check() { # check NAME OK DETAIL
  local line="PASS $1: $3"
  if [ "$2" != 0 ]; then line="FAIL $1: $3"; failed=1; fi
  echo "$line" | tee -a "$output/checks.txt"
}
check system 0 "macOS $(sw_vers -productVersion) $(uname -m)"

work=$(mktemp -d); mount=""
name="Battlefront Eternal.app"
dmg=$(ls "$packages"/*.dmg 2>/dev/null | head -1)
archive=$(ls "$packages"/macos-*-unsigned-build-input.tar.zst 2>/dev/null | head -1)
if [ -n "$dmg" ]; then
  notarized=1
  mount="$work/volume"; mkdir "$mount"
  hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$dmg" 2>/dev/null
  check "disk image opens" $? "$(basename "$dmg")"
  check "link to Applications" "$([ "$(readlink "$mount/Applications")" = /Applications ]; echo $?)" "$(readlink "$mount/Applications")"
  source="$mount/$name"
else
  notarized=0
  command -v zstd > /dev/null || brew install zstd > /dev/null 2>&1
  zstd -dc "$archive" | tar -x -C "$work"
  check "build input unpacks" $? "$(basename "$archive")"
  source="$work/$name"
fi

# What Finder does when the player drags the app to Applications.
app="/Applications/$name"
rm -rf "$app"
ditto "$source" "$app"
check "copied to Applications" $? "$app"
[ -n "$mount" ] && hdiutil detach -quiet "$mount" 2>/dev/null
# The mark every browser download carries: flags, time, the downloading app, an id.
xattr -r -w com.apple.quarantine "0083;$(printf %x "$(date +%s)");Safari;$(uuidgen)" "$app"
check "marked as a download" "$(xattr -p com.apple.quarantine "$app" > /dev/null 2>&1; echo $?)" "$(xattr -p com.apple.quarantine "$app" 2>&1)"

codesign --verify --deep --strict "$app" > "$output/codesign.log" 2>&1
check "signature verifies" $? "$(codesign --display --verbose=2 "$app" 2>&1 | grep -E '^(Authority|Signature)=' | head -1)"
verdict=$(spctl --assess --type execute --verbose=2 "$app" 2>&1); accepted=$?
if [ "$notarized" = 1 ]; then
  check "Gatekeeper accepts the download" "$accepted" "$(echo "$verdict" | tr '\n' ' ')"
  xcrun stapler validate "$app" > "$output/stapler.log" 2>&1
  check "notarization ticket stapled" $? "$(tail -1 "$output/stapler.log")"
else
  check "Gatekeeper verdict (ad-hoc build input, refusal expected)" 0 "$(echo "$verdict" | tr '\n' ' ')"
  xattr -r -d com.apple.quarantine "$app"
fi

bin="$app/Contents/MacOS"
missing=$(otool -L "$bin/bfe-client" "$bin/bfeserver" | awk '/^\t/ {print $1}' | grep -v '^@' | while read -r lib; do
  # System libraries live in the dyld shared cache, not on disk: ask the loader.
  case "$lib" in /usr/lib/*|/System/*) ;; *) echo "$lib";; esac; done)
check "only system libraries linked" "$([ -z "$missing" ]; echo $?)" "${missing:-all from the system}"
version=$("$bin/bfe-client" --version 2>&1 < /dev/null)
check "client starts" $? "$version"
check "version" 0 "$(tr -d '\n ' < "$bin/resources/bfe-release.json")"
check "licence and notices" "$([ -f "$bin/resources/legal/LICENSE" ] && [ -f "$bin/resources/legal/THIRD-PARTY-NOTICES.md" ]; echo $?)" "$app/Contents/Resources/legal"
server=$(BFE_STATE_DIR="$work/state" "$bin/bfeserver" --version 2>&1 < /dev/null)
check "server starts" $? "$server"
tools=$("$bin/resources/toolchain/python/bin/python3" -B -c 'import numpy, scipy, soundfile, soxr, PIL; print("Python tools load")' 2>&1)
check "bundled Python and its packages" $? "$tools"
ffmpeg=$("$bin/resources/toolchain/bin/ffmpeg" -hide_banner -version 2>&1 | head -1)
check "bundled FFmpeg" $? "$ffmpeg"

python3 "$repo/tools/platform-smoke-native.py" --client-bin "$bin/bfe-client" --output-dir "$output/app" > "$output/app.log" 2>&1
check "platform smoke, installed app" $? "$(tail -c 300 "$output/app.log" | tr '\n' ' ')"

codesign --verify --deep --strict "$app" >> "$output/codesign.log" 2>&1
check "signature still verifies after running" $? "nothing was written into the bundle"

rm -rf "$app" "$work"
check "removed" "$([ ! -e "$app" ]; echo $?)" "$app"
exit $failed
