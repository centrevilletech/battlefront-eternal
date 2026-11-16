#!/usr/bin/env bash
# package-smoke-linux.sh PACKAGES OUTPUT: install a Release run's Debian package
# with apt (the distribution's own repositories only), check every library the
# client loads is there, run the platform smoke on the installed client and on
# the AppImage, start the server from its archive, then purge the package and
# check nothing of it is left. Writes checks.txt and the smoke output to OUTPUT.
set -uo pipefail
packages=$(cd "$1" && pwd); output=$2
mkdir -p "$output"; output=$(cd "$output" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
failed=0
check() { # check NAME OK DETAIL
  if [ "$2" = 0 ]; then echo "PASS $1: $3"; else echo "FAIL $1: $3"; failed=1; fi | tee -a "$output/checks.txt"
}
deb=$(ls "$packages"/*.deb | head -1)
appimage=$(ls "$packages"/*.AppImage | head -1)
server=$(ls "$packages"/battlefront-eternal-server_*.tar.xz | head -1)
check system 0 "$(. /etc/os-release; echo "$PRETTY_NAME") $(uname -m), $(ldd --version | head -1)"

sudo apt-get update -q > /dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q "$deb" > "$output/apt-install.log" 2>&1
check "apt install" $? "$(basename "$deb")"
app="/opt/Battlefront Eternal"
missing=$(ldd "$app/bfe-client" 2>/dev/null | grep 'not found')
check "linked libraries" "$([ -z "$missing" ]; echo $?)" "${missing:-all found}"
# Libraries the client opens at run time rather than links.
absent=""
for lib in libEGL.so.1 libGL.so.1 libwayland-client.so.0 libwayland-cursor.so.0 libwayland-egl.so.1 \
           libxkbcommon.so.0 libxkbcommon-x11.so.0 libX11.so.6 libX11-xcb.so.1 libXcursor.so.1 libXrandr.so.2 libXi.so.6; do
  ldconfig -p | grep -q " $lib " || absent="$absent $lib"
done
check "run-time libraries" "$([ -z "$absent" ]; echo $?)" "${absent:-all present}"
check "version" 0 "$(cat "$app/resources/bfe-release.json" | tr -d '\n ')"
check "licence and notices" "$([ -f "$app/resources/legal/LICENSE" ] && [ -f "$app/resources/legal/THIRD-PARTY-NOTICES.md" ]; echo $?)" "$app/resources/legal"
check "launchers" "$([ -x /usr/bin/battlefront-eternal ] && [ -x /usr/bin/bfeserver ] && [ -f /usr/share/applications/battlefront-eternal.desktop ]; echo $?)" "/usr/bin, desktop entry"

# Xvfb and Mesa's llvmpipe are the test's own display, not the package's.
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends xvfb xauth libgl1-mesa-dri > /dev/null 2>&1
python3 "$repo/tools/platform-smoke-native.py" --client-bin "$app/bfe-client" --output-dir "$output/deb" > "$output/deb.log" 2>&1
check "platform smoke, installed package" $? "$(tail -c 300 "$output/deb.log" | tr '\n' ' ')"

mkdir -p "$output/appimage-run"; cp "$appimage" "$output/appimage-run/"; chmod +x "$output/appimage-run/"*.AppImage
(cd "$output/appimage-run" && ./*.AppImage --appimage-extract > /dev/null)
APPIMAGE="$(ls "$output/appimage-run/"*.AppImage)" python3 "$repo/tools/platform-smoke-native.py" \
  --client-bin "$output/appimage-run/squashfs-root/AppRun" --output-dir "$output/appimage" > "$output/appimage.log" 2>&1
check "platform smoke, AppImage" $? "$(tail -c 300 "$output/appimage.log" | tr '\n' ' ')"
rm -rf "$output/appimage-run"

# The dedicated server from its archive runs on this system (a battle needs prepared game data).
srv=$(mktemp -d); tar -C "$srv" -xJf "$server"
version=$(BFE_STATE_DIR="$srv/state" "$srv/bfeserver" --version 2>&1 < /dev/null)
check "server archive runs" $? "$version"
rm -rf "$srv"

sudo DEBIAN_FRONTEND=noninteractive apt-get purge -y -q battlefront-eternal > "$output/apt-purge.log" 2>&1
check "apt purge" $? ""
left=$(ls -d "$app" /usr/bin/battlefront-eternal /usr/bin/bfe-client /usr/bin/bfeserver /usr/share/applications/battlefront-eternal.desktop 2>/dev/null)
check "nothing left after purge" "$([ -z "$left" ]; echo $?)" "${left:-clean}"
exit $failed
