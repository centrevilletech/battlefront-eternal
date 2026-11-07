#!/usr/bin/env bash
# Run after building bfe-client, its tests and bfe-native. Keep captures outside
# the repository. The display must be a real window system with the same GPU
# used by the headless EGL device. Only summaries leave this script.
set -euo pipefail
capture=${1:?usage: tools/native-client-gate.sh CAPTURE_DIR [SECONDS]}
seconds=${2:-20}
bin=${BFE_NATIVE_BIN:-rust/target/debug}
testbin=${BFE_NATIVE_TEST_BIN:-$bin}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cat > "$work/alsa.conf" <<'ALSA'
pcm.!default { type null }
ctl.!default { type hw card 0 }
ALSA
export ALSA_CONFIG_PATH="$work/alsa.conf" BFE_AUDIO_NULL=1
# Use the existing test executables: compilation belongs to the build job.
for family in host audio steady_state capture; do
    found=0
    for exe in "$testbin"/deps/"$family"-*; do
        if [[ -f $exe && -x $exe && $exe != *.so ]]; then
            "$exe"
            if [[ $family == audio ]]; then "$exe" --ignored; fi
            found=1
        fi
    done
    [[ $found == 1 ]] || { echo "missing $family test executable" >&2; exit 1; }
done
# Metadata sizes make framebuffer hashes independent of the window manager.
read -r width height samples < <(python3 - "$capture" <<'PY'
import json, pathlib, sys
m = json.loads((pathlib.Path(sys.argv[1]) / 'calls.json').read_text())
print(*m.get('size', [1600, 900]), m.get('samples', 2))
PY
)
W=$width H=$height SAMPLES=$samples SECS=1 HASH_FRAME=0 "$bin/bfe-native" "$capture" > "$work/headless.log"
python3 tools/native-client-window.py "$bin/bfe-client" "$capture" --seconds "$seconds" --hash-frame 0 > "$work/window.log"
cat "$work/headless.log" "$work/window.log"
reference=$(awk '/^frame_hash / {print $3}' "$work/headless.log")
candidate=$(awk '/^frame_hash / {print $3}' "$work/window.log")
[[ -n $reference && $reference == "$candidate" ]] || { echo 'native frame hashes differ' >&2; exit 1; }
# Equality alone would also accept a cleared target. Require actual world or
# actor submissions, and prove the audio callback ran during the window test.
python3 - "$work/window.log" <<'PY_CHECK'
import pathlib, re, sys
log = pathlib.Path(sys.argv[1]).read_text()
draw = re.search(r'window draw world (\d+) instances (\d+) GL calls (\d+)', log)
assert draw and sum(map(int, draw.groups()[:2])) > 0, 'no scene draws'
audio = re.search(r'audio_blocks (\d+)', log)
assert audio and int(audio[1]) > 0, 'no audio callback'
clock = re.search(r'window frames (\d+) seconds ([\d.]+) world_ticks (\d+)', log)
assert clock and int(clock[3]) > 0, 'world clock did not advance'
assert int(clock[1]) / float(clock[2]) < 80, 'virtual display redraw was not paced'
PY_CHECK
echo 'native window/headless hash equality: PASS'
