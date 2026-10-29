# Dedicated server updates

A dedicated game server updates itself the way the desktop client does
([desktop-updates.md](desktop-updates.md)): it follows the same signed
release channel, so it always runs the engine and server adapter of the
newest release, on game data that release's own tools prepared, which is
what every player's client of that release has. Nobody deploys by hand,
and a server never lags behind the players it is meant for.
`electron/update/server.js` does it; every release carries it (the host
runtime's `update/`, `build/prepare-host-runtime.mjs`).

## What a run does

A systemd timer (`bfeserver-update.timer`) runs the updater five minutes
after the last run ended, at idle CPU and disk priority:

1. **Check** the channel (`development-preview`). Nothing happens unless a
   release is newer than the one running and has not failed here.
2. **Install** it into `<root>/versions/<version>/`: of the release's Linux
   build only `resources/host-runtime/` (engine, server adapter, this
   updater), `resources/tools/` and `resources/toolchain/`, each file
   checked against the signed manifest. Files an installed version already
   has are hard-linked, so a typical release downloads a few megabytes.
3. **Prepare its game data** from the owned game at `retail` (the folder
   holding `GameData`): the release's `tools/prepare.py --core-only`, run by
   its own toolchain as the server's user into an empty folder,
   `<root>/data/<key>`. The key is a hash of the tools and the Python they
   run on, so releases that change neither share the data, and data is never
   refreshed in place (a stage reusing old steps once served stale sides to
   every new player; [desktop.md](desktop.md)). On a 4-core VM with one
   job this takes about 45 minutes, while the server keeps playing. A run
   whose preparation fails three times for the same key gives up until a
   newer release.
4. **Check** the release: its engine's network contract loads, and every
   battle in the server's rotation fingerprints on the new data
   (`src/runtime/content-hash.js`).
5. **Swap, once nobody plays.** The player count comes from the game port's
   `/info`. A server between battles does not answer there while its
   players wait, so an unknown count waits too; a server that is down or in
   a fault has nobody on it. With nobody on, the updater stops the service,
   points `<root>/engine`, `server`, `game-data` and `update` at the new
   release, starts it and waits up to three minutes for health: running,
   multiplayer ready, the release's network contract, ticks advancing. If
   it fails, every link goes back, the old release starts again, and the new
   one is set aside until a newer one arrives. With players on, the next
   run looks again; the work of steps 2 to 4 is kept. `<root>/update` moves
   to the release's updater only once that answers `--status` through a
   link, as the timer starts it; otherwise the running updater stays, so a
   broken one can never leave the server without updates.
6. **Clean up**: versions and game data neither the running release nor
   the one before it uses are deleted. The first release replaces a manual
   deploy's folders, which stay as `<name>.manual` until the release after
   it, to roll back to.

`<root>/update-state.json` records the running and previous versions, a
release waiting for an empty server, set-aside versions, preparation
failures and the last error. `node <root>/update/server.js --status`
prints it. Everything a run does is logged to the journal:
`journalctl -u bfeserver-update`.

## Layout

| Path | What |
| --- | --- |
| `<root>/engine`, `server`, `update` | Links into the running version's `resources/host-runtime/` |
| `<root>/instances/<name>/` | Another server's `engine`, `server` and `game-data` links |
| `<root>/game-data` | Link to `<root>/data/<key>` |
| `<root>/versions/<version>/` | Installed releases (`.bfe-install.json` written last) |
| `<root>/data/<key>/` | Prepared game data, owned by root, readable by the server |
| `<root>/retail/` | The owned game, read only by the preparation |
| `/etc/bfeserver-update.json` | Configuration |

The server's own configuration (`/var/lib/bfeserver/config.json`) keeps
naming `<root>/engine` and `<root>/game-data`; the links move under it.

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
`instances` lists them; the first, `main`, is the one the defaults above
describe:

```json
"instances": [
  { "name": "main" },
  { "name": "duels", "healthPort": 18453, "gamePort": 18452 }
]
```

Every other server gets `bfeserver-<name>.service`, its state in
`/var/lib/bfeserver-<name>` (its `config.json`, naming
`<root>/instances/<name>/engine` and `game-data`, and the identity it
creates) and its links in `<root>/instances/<name>/`. `service`,
`stateDir`, `links`, `health`, `info` and `memoryMax` (1.5 GB) override
those. `--install-units` writes and enables the units and makes the
folders; the next run starts a server once its `config.json` is there.
Each server moves to a new release on its own, once nobody plays on it, so
one busy server never holds another back; the data and the version
folders are shared, and a version stays installed while any server runs
it. A server's game port (`gamePort`, UDP and TCP) must be open to
players.

## Setting up a server

On a Linux server with systemd, `bfeserver.service` running from
`<root>/server` with `<root>/node`, as root:

1. Copy the owned game to `<root>/retail` (the folder that holds
   `GameData`), readable by the server's user: `chmod -R a+rX`.
2. Copy `electron/update/` from an engine checkout (the files
   `build/prepare-host-runtime.mjs` lists, with a `package.json` of
   `{"type": "module"}`) to `<root>/update-bootstrap`, and link
   `<root>/update` to it.
3. `<root>/node/bin/node <root>/update/server.js --install-units` writes
   `bfeserver-update.service` and `.timer`, a configuration if there is
   none, and enables the timer.

The first run installs the newest release, prepares its data, and takes
over from the manual deploy once nobody plays.
