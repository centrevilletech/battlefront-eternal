#!/bin/sh
# install-server.sh: install the Battlefront Eternal dedicated server (bfeserver) from this unpacked
# archive, or move a server that runs the earlier Node.js server to it. Run as root:
#
#   sudo ./install-server.sh [--game DIR] [--root DIR] [--state DIR] [--now]
#
#   --game DIR   the owned game (the folder holding GameData): game data is prepared from it when
#                a server has none, or has data older tools prepared
#   --root DIR   where releases and prepared data go (default /opt/bfeserver)
#   --state DIR  for a new server: its state folder (default /var/lib/bfeserver)
#   --now        restart servers even while players are on (they are dropped)
#
# Every systemd unit that runs the dedicated server (bfeserver*.service) keeps its name, user,
# state folder (config.json, identity, operator token, bans) and ports: a drop-in only changes
# what it starts, <root>/rust/current/bfeserver. A server with nobody on it restarts at once; one
# with players switches at its next restart. A machine whose servers follow the release channel
# with the updater (/etc/bfeserver-update.json) needs nothing: the updater moves them itself.
# Without any server unit, a new bfeserver.service is set up. Running it again with a newer
# archive updates the servers it set up. Undo for a moved server: remove
# /etc/systemd/system/<unit>.service.d/50-bfeserver.conf and restart the unit.
set -eu

ROOT=/opt/bfeserver
GAME=
NEW_STATE=/var/lib/bfeserver
NOW=0
while [ $# -gt 0 ]; do
  case $1 in
    --game) GAME=${2:?--game needs a folder}; shift 2 ;;
    --root) ROOT=${2:?--root needs a folder}; shift 2 ;;
    --state) NEW_STATE=${2:?--state needs a folder}; shift 2 ;;
    --now) NOW=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
done

HERE=$(cd "$(dirname "$0")" && pwd)
UNITS=${UNITS:-/etc/systemd/system}
say() { echo "== $*"; }
run() { echo "+ $*"; "$@"; }
die() { echo "!! $*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "run as root (sudo)"
[ -x "$HERE/bfeserver" ] && [ -f "$HERE/bfe-tree.json" ] || die "run this from the unpacked server archive"
command -v systemctl >/dev/null || die "systemd is needed (for a container, see the archive's README)"
PY=$HERE/toolchain/python/bin/python3
VERSION=$("$PY" -c 'import json,sys; t=json.load(open(sys.argv[1])); print(t.get("version") or "")' "$HERE/bfe-tree.json")
[ -n "$VERSION" ] || VERSION=$(date -u +%Y%m%d%H%M%S)

# A JSON value from a file: jvalue FILE KEY
jvalue() { "$PY" -c 'import json,sys
try: v=json.load(open(sys.argv[1])).get(sys.argv[2])
except Exception: v=None
print("" if v is None else v)' "$1" "$2"; }

if [ -f /etc/bfeserver-update.json ] && systemctl is-enabled -q bfeserver-update.timer 2>/dev/null; then
  if [ ! -e "$ROOT/rust/current" ]; then
    say "the servers here follow the release channel (bfeserver-update.timer): the updater moves them to"
    say "bfeserver by itself once nobody plays. To check now: systemctl start bfeserver-update.service;"
    say "journalctl -u bfeserver-update shows what it did."
    exit 0
  fi
fi

say "1. install $VERSION into $ROOT/rust/$VERSION"
DEST=$ROOT/rust/$VERSION
if [ -x "$DEST/bfeserver" ] && cmp -s "$HERE/bfeserver" "$DEST/bfeserver"; then
  echo "already installed"
else
  run mkdir -p "$ROOT/rust"
  run rm -rf "$DEST.new"
  run cp -a "$HERE" "$DEST.new"
  run chown -R root:root "$DEST.new"
  run chmod -R a+rX,go-w "$DEST.new"
  run rm -rf "$DEST"
  run mv "$DEST.new" "$DEST"
fi
run ln -sfn "$VERSION" "$ROOT/rust/current.new"
run mv -T "$ROOT/rust/current.new" "$ROOT/rust/current"

# The servers: units that run the Node.js server (src/main.js --serve) or bfeserver.
FOUND=$(grep -l -E '^ExecStart=.*(src/main\.js|bfeserver)[^ ]* +--serve' "$UNITS"/bfeserver*.service 2>/dev/null || true)
if [ -z "$FOUND" ]; then
  say "2. no server unit found: setting up bfeserver.service with state in $NEW_STATE"
  id bfeserver >/dev/null 2>&1 || run useradd --system --home-dir "$NEW_STATE" --shell /usr/sbin/nologin bfeserver
  run mkdir -p "$NEW_STATE"
  run chown bfeserver:bfeserver "$NEW_STATE"
  run chmod 700 "$NEW_STATE"
  cat > "$UNITS/bfeserver.service" <<EOF
[Unit]
Description=Battlefront Eternal dedicated server
Wants=network-online.target
After=network-online.target

[Service]
User=bfeserver
Group=bfeserver
Environment=BFE_STATE_DIR=$NEW_STATE
ExecStart=$ROOT/rust/current/bfeserver --serve
Restart=always
RestartSec=5
MemoryMax=2800M
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$NEW_STATE
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF
  echo "+ wrote $UNITS/bfeserver.service"
  FOUND=$UNITS/bfeserver.service
  NEW=1
else
  NEW=0
  say "2. servers found: $(for f in $FOUND; do basename "$f"; done | tr '\n' ' ')"
fi

# The game data each server uses: kept when tools of this version's content format prepared it.
current_data() { # current_data DIR: true when DIR holds data in this release's content format
  "$PY" - "$1" "$HERE/tools/content-version.json" <<'PY'
import json, sys
from pathlib import Path
data, want = Path(sys.argv[1]), json.load(open(sys.argv[2]))
try: have = json.loads((data / ".prepare.json").read_text())
except Exception: sys.exit(1)
ok = (data / "maps").is_dir() and have.get("mapImportVersion") == want["maps"] and not have.get("failed")
sys.exit(0 if ok else 1)
PY
}
PREPARED=
prepare_data() { # prepare once per run into $ROOT/data/$VERSION
  [ -n "$PREPARED" ] && return 0
  [ -n "$GAME" ] || { [ -f /etc/bfeserver-update.json ] && GAME=$(jvalue /etc/bfeserver-update.json retail); }
  [ -n "$GAME" ] && [ -d "$GAME/GameData" ] || die "game data must be prepared: give --game <folder holding GameData>"
  OUT=$ROOT/data/$VERSION
  if current_data "$OUT"; then PREPARED=$OUT; return 0; fi
  say "preparing game data from $GAME into $OUT (several minutes; the servers keep playing)"
  run rm -rf "$OUT.staging"
  run mkdir -p "$OUT.staging"
  (cd "$HERE/tools" && BFE_GAME_DIR=$GAME BFE_CONTENT_DIR=$OUT.staging KTX2_ENCODER=off PYTHONDONTWRITEBYTECODE=1 \
    PATH="$HERE/toolchain/bin:$PATH" "$PY" -u prepare.py --core-only > "$OUT.staging/prepare.log" 2>&1) \
    || die "preparation failed: see $OUT.staging/prepare.log"
  current_data "$OUT.staging" || die "preparation incomplete: see $OUT.staging/prepare.log"
  run chmod -R a+rX,go-w "$OUT.staging"
  run rm -rf "$OUT"
  run mv "$OUT.staging" "$OUT"
  PREPARED=$OUT
}

say "3. each server: drop-in, data, restart when empty"
CHANGED_UNITS=
for unit_file in $FOUND; do
  unit=$(basename "$unit_file" .service)
  env_of() { systemctl show -p Environment --value "$unit" | tr ' ' '\n' | sed -n "s/^$1=//p" | tail -1; }
  state=$(env_of BFE_STATE_DIR); [ -n "$state" ] || state=$NEW_STATE
  hport=$(env_of PORT); [ -n "$hport" ] || hport=18450
  cfg=$state/config.json
  data=$(env_of BFE_DATA_DIR); [ -n "$data" ] || { [ -f "$cfg" ] && data=$(jvalue "$cfg" data); } || true
  gport=18451; [ -f "$cfg" ] && [ -n "$(jvalue "$cfg" gamePort)" ] && gport=$(jvalue "$cfg" gamePort)
  echo "$unit: state $state, health :$hport, game :$gport, data ${data:-none}"
  data_line=
  if [ -n "$data" ] && current_data "$data"; then
    echo "$unit: its game data is in this release's format: kept"
  elif [ -n "$data" ] || [ -n "$GAME" ] || [ "$NEW" = 1 ]; then
    prepare_data
    data_line="Environment=BFE_DATA_DIR=$PREPARED"
    echo "$unit: plays on $PREPARED (its config.json keeps its own \"data\"; the drop-in overrides it)"
  fi
  dropin=$UNITS/$unit.service.d/50-bfeserver.conf
  mkdir -p "$(dirname "$dropin")"
  if [ "$NEW" = 1 ]; then
    printf '[Service]\n%s\n' "$data_line" > "$dropin.new"
  else
    printf '# Written by install-server.sh: this server runs bfeserver. Remove this file and restart the\n# unit to run what the unit itself names again.\n[Service]\nWorkingDirectory=%s\nExecStart=\nExecStart=%s --serve\n%s\n' \
      "$ROOT/rust/current" "$ROOT/rust/current/bfeserver" "$data_line" > "$dropin.new"
  fi
  if cmp -s "$dropin.new" "$dropin"; then rm -f "$dropin.new"; echo "unchanged: $dropin"
  else mv "$dropin.new" "$dropin"; echo "+ wrote $dropin"; CHANGED_UNITS="$CHANGED_UNITS $unit"; fi
done
run systemctl daemon-reload

for unit_file in $FOUND; do
  unit=$(basename "$unit_file" .service)
  hport=$(systemctl show -p Environment --value "$unit" | tr ' ' '\n' | sed -n 's/^PORT=//p' | tail -1); [ -n "$hport" ] || hport=18450
  state=$(systemctl show -p Environment --value "$unit" | tr ' ' '\n' | sed -n 's/^BFE_STATE_DIR=//p' | tail -1)
  gport=18451; [ -n "$state" ] && [ -f "$state/config.json" ] && [ -n "$(jvalue "$state/config.json" gamePort)" ] && gport=$(jvalue "$state/config.json" gamePort)
  if [ "$NEW" = 1 ] && [ ! -f "$state/config.json" ]; then
    run systemctl enable "$unit"
    say "$unit is set up but has no configuration yet: run"
    say "  sudo -u bfeserver BFE_STATE_DIR=$state $ROOT/rust/current/bfeserver"
    say "to name it, choose its playlist and listing, then: systemctl start $unit"
    continue
  fi
  players=$(curl -s -m 3 "http://127.0.0.1:$gport/info" 2>/dev/null | "$PY" -c 'import json,sys
try: print(json.load(sys.stdin).get("players", ""))
except Exception: print("")')
  pid=$(systemctl show -p MainPID --value "$unit")
  case " $CHANGED_UNITS " in *" $unit "*) same=0 ;; *) same=1 ;; esac
  if [ "$same" = 1 ] && [ "${pid:-0}" != 0 ] && [ "$(readlink -f "/proc/$pid/exe")" = "$(readlink -f "$DEST/bfeserver")" ]; then
    say "$unit already runs bfeserver $VERSION: not restarted"
    continue
  fi
  if [ -n "$players" ] && [ "$players" != 0 ] && [ "$NOW" != 1 ]; then
    say "$unit: $players playing; it runs bfeserver from its next restart (systemctl restart $unit)"
    continue
  fi
  systemctl is-enabled -q "$unit" || run systemctl enable "$unit"
  run systemctl restart "$unit"
  ok=0; i=0
  while [ $i -lt 90 ]; do
    h=$(curl -s -m 3 "http://127.0.0.1:$hport/healthz" 2>/dev/null || true)
    case $h in *'"status":"running"'*'"multiplayerReady":true'*|*'"multiplayerReady":true'*'"status":"running"'*) ok=1; break ;; esac
    case $h in *'"status":"fault"'*|*'"status":"setup"'*) break ;; esac
    i=$((i + 1)); sleep 2
  done
  echo "$unit /healthz: ${h:-no answer}"
  [ "$ok" = 1 ] && say "$unit runs bfeserver $VERSION" || say "$unit is NOT healthy: journalctl -u $unit"
done
say "done. Status: systemctl status $(for f in $FOUND; do basename "$f" .service; done | tr '\n' ' ')"
