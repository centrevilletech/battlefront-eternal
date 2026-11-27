# Dedicated server updates

A dedicated game server can update itself the way the desktop client does
([desktop-updates.md](desktop-updates.md)): it follows the same signed
release channel, so it runs the `bfeserver` of the newest release, on game
data that release's own tools prepared, which is what every player's client of
that release has. A server set up without the updater is updated with the
server archive's `install-server.sh` instead (below).

## The updater

The updater is a small Node.js program (Node 22.13 or later, at
`<root>/node`), the one servers have run since before `bfeserver`. Every
Linux release carries it in `resources/host-runtime/update/`, beside what it
installs, so each release brings the updater that installs the next one. A
systemd timer (`bfeserver-update.timer`) runs it five minutes after the last
run ended, at idle CPU and disk priority:

1. **Check** the channel (`development-preview`). Nothing happens unless a
   release is newer than the one running and has not failed here.
2. **Install** it into `<root>/versions/<version>/`: of the release's Linux
   build only `resources/host-runtime/`, `resources/tools/` and
   `resources/toolchain/`, each file checked against the signed manifest.
   Files an installed version already has are hard-linked, so a typical
   release downloads a few megabytes.
3. **Prepare its game data** from the owned game at `retail` (the folder
   holding `GameData`): the release's `tools/prepare.py --core-only`, run by
   its own toolchain as the server's user into an empty folder,
   `<root>/data/<key>`. The key is a hash of the tools and the Python they
   run on, so releases that change neither share the data, and data is never
   refreshed in place. On a 4-core VM with one job this takes about 45
   minutes, while the server keeps playing. A run whose preparation fails
   three times for the same key gives up until a newer release.
4. **Check** the release: its network contract (`bfeserver --contract`) and
   that every map of each server's rotation is prepared.
5. **Swap, once nobody plays.** The player count comes from the game port's
   `/info`; a server that is down or in a fault has nobody on it. With nobody
   on, the updater stops the service, points `<root>/server`, `<root>/engine`
   and `<root>/game-data` at the new release and its data, starts it and waits
   up to three minutes for health on `/healthz`: running, multiplayer ready,
   the release's network contract, ticks advancing. If it fails, every link
   goes back, the old release starts again, and the new one is set aside until
   a newer one arrives. With players on, the next run looks again; the work of
   steps 2 to 4 is kept. After a swap the timer runs the new release's
   updater (`<root>/update`).
6. **Clean up**: versions and game data neither the running release nor
   the one before it uses are deleted.

The server's unit starts `<root>/node/bin/node <root>/server/src/main.js
--serve`: in a release, `server/src/main.js` is a launcher that runs the
`bfeserver` beside it (`server/bfeserver`) with the same arguments and
environment and passes the stop signal on. Units and updaters set up for
the earlier Node.js server therefore run `bfeserver` from their first update
to a release that has it, with their configuration, identity, operator
token, bans and ports.

`<root>/update-state.json` records the running and previous versions, a
release waiting for an empty server, set-aside versions, preparation
failures and the last error. Everything a run does is logged to the journal:
`journalctl -u bfeserver-update`; `<root>/node/bin/node <root>/update/server.js
--status` prints the state.

## The server archive

Hosts that run the server without the updater get the server's files on
their own: every release publishes
`battlefront-eternal-server_<version>_linux-amd64.tar.xz` and
`_linux-arm64.tar.xz` beside the `.deb` in
`https://get.battlefronteternal.com/downloads/latest/`, listed in that
folder's `SHA256SUMS` (signed, `SHA256SUMS.asc`). The `<version>` is the
package version, `0.1.0~preview.N`. `build/native/server-archive.py` packs it
from the staged Linux build the `.deb` is made of, so an archive and its
package hold the same files, and the release host refuses a release whose
archive reaches outside this layout:

```
bfeserver   install-server.sh   tools/   toolchain/   legal/   bfe-tree.json
```

`sudo ./install-server.sh [--game DIR]` in the unpacked archive installs it:

- it copies the archive to `/opt/bfeserver/rust/<version>/` and points
  `/opt/bfeserver/rust/current` at it;
- every systemd unit `bfeserver*.service` that runs a dedicated server (the
  earlier Node.js server's `src/main.js --serve`, or `bfeserver --serve`) gets
  a drop-in, `50-bfeserver.conf`, that starts `rust/current/bfeserver` instead:
  the unit keeps its name, user, state folder and limits, and the server its
  configuration, identity, operator token, bans and ports;
- a server whose game data is not in the release's content format gets data
  prepared from `--game` (the folder holding `GameData`) in
  `/opt/bfeserver/data/<version>`, named by `BFE_DATA_DIR` in the drop-in;
- each server nobody plays on restarts at once; a busy one moves at its next
  restart (`--now` restarts it anyway);
- without any server unit it sets up `bfeserver.service` (user `bfeserver`,
  state in `/var/lib/bfeserver`, or `--state DIR`) and says how to configure
  it with `bfeserver`'s setup menu.

A machine whose servers follow the channel with the updater needs none of it:
the script says so and stops. Running it again with a newer archive updates
the servers it set up. Removing a drop-in and restarting the unit puts that
server back on what its unit names.

The archive can also run without systemd (a container, LinuxGSM): with it
unpacked into `<bfe>`, prepare the 2004 data into `<data>` and start the server
pointing at `<data>` (2005 preparation by hand is not supported yet):

```sh
BFE_FAMILY=swbf1 BFE_GAME_DIR=<retail> BFE_CONTENT_DIR=<data> <bfe>/toolchain/python/bin/python3 <bfe>/tools/prepare.py --core-only
BFE_DATA_DIR=<data> BFE_STATE_DIR=<state> <bfe>/bfeserver --serve
```

The data only needs preparing again when a release changes `tools/` or
`toolchain/`: compare their entries in `bfe-tree.json`.

## Layout

| Path | What |
| --- | --- |
| `<root>/server`, `<root>/engine` | Links into the running version's `resources/host-runtime/` |
| `<root>/update` | Link to the updater the timer runs (a version's `resources/host-runtime/update`) |
| `<root>/game-data` | Link to `<root>/data/<key>` |
| `<root>/instances/<name>/` | Another server's `server`, `engine` and `game-data` links |
| `<root>/versions/<version>/` | Installed releases (`.bfe-install.json` written last) |
| `<root>/data/<key>/` | Prepared game data (flat root for `swbf1`, `swbf2/.content/` subfolder for `swbf2`), owned by root, readable by the server |
| `<root>/retail/` | The owned game, read only by the preparation |
| `<root>/node/` | Node.js, which runs the updater and the launcher |
| `/etc/bfeserver-update.json` | Configuration |

The server's own configuration (`/var/lib/bfeserver/config.json`) keeps
naming `<root>/game-data`; the links move under it.

## Configuration

`/etc/bfeserver-update.json` overrides any of these defaults:

```json
{
  "channel": "development-preview",
  "root": "/opt/bfeserver",
  "stateDir": "/var/lib/bfeserver",
  "service": "bfeserver",
  "user": "bfeserver",
  "retail": "/opt/bfeserver/retail",
  "jobs": 2,
  "health": "http://127.0.0.1:18450/healthz",
  "info": "http://127.0.0.1:18451/info",
  "hold": false
}
```

`hold: true` pauses updates (to keep a hand-deployed build running, say).
`jobs` is how many maps prepare at once, about 0.6 GB each; the unit caps
the updater and its preparation at 1.8 GB so the game servers are never
starved.

`rebootWhenRequired: true` restarts the machine when its own updates ask
for it (`/var/run/reboot-required`: unattended upgrades installed a kernel
or a C library), once every server is empty and at most once in six hours.
The servers' units and the timer are enabled, so all of it comes back by
itself. Exempt the servers from needrestart
(`/etc/needrestart/conf.d/bfeserver.conf`:
`$nrconf{override_rc}->{qr(^bfeserver)} = 0;`), or an upgrade restarts
them while people play.

## More than one server

A machine can run several servers on one installation, each with its own
name, playlist, rules, ports and identity, all on the same release.
`instances` lists them; the default, `main`, is the one the defaults above
describe:

```json
"instances": [
  { "name": "main" },
  { "name": "duels", "healthPort": 18453, "gamePort": 18452 }
]
```

Every other server gets `bfeserver-<name>.service`, its state in
`/var/lib/bfeserver-<name>` (its `config.json`, naming
`<root>/instances/<name>/game-data`, and the identity it creates) and its links
in `<root>/instances/<name>/`. `service`, `stateDir`, `links`, `health`, `info`
and `memoryMax` (1.5 GB) override those. Each server moves to a new release on
its own, once nobody plays on it, so one busy server never holds another back;
the data and the version folders are shared, and a version stays installed
while any server runs it. A server's game port (`gamePort`, UDP and TCP) must
be open to players. LAN discovery (`discoveryPort`, 18452 by default) must be
off (`"discovery": false`) on servers whose game ports include 18452.

## Setting up a server with automatic updates

On a Linux server with systemd, as root, with `<root>` = `/opt/bfeserver`:

1. Copy the owned game to `<root>/retail` (the folder that holds
   `GameData`), readable by the server's user: `chmod -R a+rX`.
2. Unpack Node.js 22.13 or later into `<root>/node`.
3. Copy `resources/host-runtime/update/` of a release (its Linux `.deb`,
   AppImage or installed client) to `<root>/update-bootstrap`, and link
   `<root>/update` to it.
4. Write the main server's unit, `bfeserver.service` (user `bfeserver`,
   `Environment=BFE_STATE_DIR=/var/lib/bfeserver`, `ExecStart=<root>/node/bin/node
   <root>/server/src/main.js --serve`), and its `config.json` naming
   `<root>/game-data` (`bfeserver`'s setup menu writes one).
5. `<root>/node/bin/node <root>/update/server.js --install-units` writes
   `bfeserver-update.service` and `.timer`, the units of further servers, a
   configuration if there is none, and enables the timer.

The first run installs the newest release, prepares its data, and starts the
servers once it is done.
