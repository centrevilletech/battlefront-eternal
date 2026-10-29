# Desktop updates

The desktop client updates itself when it starts on Windows (x64) and Linux
(x64 and arm64): no reinstall, no password, the same way on every
distribution. macOS is not supported yet and does not self-update
(`electron/update/state.js` lists the platforms that do). An update covers
everything in the app: Electron, the main process, the interface, the
engine, the hosting runtime and the preparation tools. Prepared game data
is never part of an update; it is refreshed only when a release changes
what preparation produces.

## On every launch

1. **Hand-off** (`electron/boot.js`, before the app loads). If the
   user's update folder holds a version newer than the executable that
   started, that version starts with the same arguments and this one
   exits once its window is up. The installed package stays as it is
   (the Debian package, the AppImage or the Windows installation) and is
   the fallback.
2. **Check** (`electron/main.js`, under the loader). The client asks the
   release feed of its channel for the newest release. Offline, or with
   the feed unreachable, it starts as it is after at most 5 seconds (a
   refused connection fails at once).
3. **Install.** Files of the new release that any build on this computer
   already has are reused (hard links on Linux, copies on Windows); the
   rest are downloaded and checked. A typical release changes `app.asar`
   and the build's file index: about 2 MB instead of a 300 MB installer.
   The loader shows the progress and a SKIP FOR NOW button. Retried
   downloads failing three times show TRY AGAIN and PLAY WITHOUT UPDATING.
4. **Restart.** The new version starts and the old one exits once the new
   window is up. About 15 seconds later the new version counts as healthy.
   Linux launchers are pointed at it, and versions nothing needs are
   deleted.
5. **Refresh game data.** Before opening the menu, check imported and local
   prepared data against the new client's content generations. Stale data is
   rebuilt automatically from the installed original game. Verified imported
   checkpoints preserve unaffected work. Without an installed source, the
   loader offers to attach it or import an updated export. A failed refresh
   stays on the loader for retry; it does not open a match with stale data.

Updates are applied only at boot, never during a match. A hosted server
keeps running from the version that started it, through updates, until it
stops; that version is kept until then.

## Where updates live

| Platform | Folder |
| --- | --- |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/battlefront-eternal` |
| Windows | `%LOCALAPPDATA%\Battlefront Eternal` |

Inside: `versions/<version>/` (complete installs, each with its
`.bfe-install.json` record written last), `state.json` (launch attempts,
failed and healthy versions), `launches/` (window-up markers), `current`
(Linux: a link to the newest healthy version) and `update.log`.

Settings, identities, prepared game data and the hosted server's state
stay in the user data folder (`~/.config/Battlefront Eternal`,
`%APPDATA%\Battlefront Eternal`), which no version owns.

The player's settings (profile name, options, bindings) are kept in
`player-settings.json` there by the main process, not in the page's
localStorage alone (`electron/player-settings.js`). Chromium locks
localStorage to one process per user data folder. An update installed
while the game runs starts the new version while the old one is still up:
the old one waits for the new window and, under Steam, stays until the
game ends. A second process gets an empty, in-memory localStorage, so the
new version used to start as PLAYER 1 with default options and lose
whatever it saved. The boot hand-off has no such problem: its parent
waits before Electron is ready and holds no lock. The first version with
the file moves localStorage's settings into it. If that version was
itself started by an update, it writes only the player's own changes
until the next launch can read localStorage again.

### Debian and Ubuntu

The package installs `/opt/Battlefront Eternal`, owned by root, with an
AppArmor profile that allows user namespaces for that executable (Ubuntu
24.04 denies them otherwise). A version started from it inherits the
profile, so Chromium's sandbox works for versions in the user's folder
too; this was verified on Ubuntu 24.04 with
`apparmor_restrict_unprivileged_userns=1`. Where user namespaces are off
entirely, the package's `chrome-sandbox` is setuid root and the started
version uses it (`CHROME_DEVEL_SANDBOX`); versions in the user's folder
never carry their own `chrome-sandbox`. `apt upgrade` still works: a newer
package simply becomes the newest version.

### AppImage

The Linux one-command installer, `install.sh`, lives in the website
repository, not in this one. It writes `~/.local/bin/battlefront-eternal`,
which ran the AppImage unpacked on every start (for FUSE 3-only systems such
as Omarchy). The app rewrites that launcher, and its own `battlefront`, to
start `current` directly when there is one, and the AppImage otherwise.
Where user namespaces are unavailable, the launcher adds `--no-sandbox`,
as the AppImage's own launcher does. A version started from an AppImage
gets none of the AppImage mount's paths (`PATH`, `LD_LIBRARY_PATH` and the
rest disappear with the mount); the AppImage stays known as
`BFE_APPIMAGE`.

### Windows

The installer is per user; updates go beside it in the local application
data folder, which needs no administrator rights. Uninstalling removes
them (`build/installer.nsh`) and keeps the game data.

## Trust

Only releases signed with a key listed in `electron/update/keys.js` are
installed. Choosing another master server or feed grants no code
execution.

- The channel file `downloads/updates/<channel>.json` is an envelope: the
  release JSON (version, release id, content generations and, per
  platform, the manifest's path and SHA-256), its Ed25519 signature and
  the key id (the first 16 hex digits of the SHA-256 of the public key's
  SubjectPublicKeyInfo).
- Each platform's manifest (`downloads/<release>/update-<platform>.json.zst`)
  lists every file with its size, SHA-256, compressed size and mode, plus
  links and empty folders. Paths and link targets must stay inside the
  version folder.
- Files come from a content-addressed store,
  `downloads/updates/blobs/<aa>/<sha256>.zst`, named by the SHA-256 of
  their contents. Each download is decompressed, hashed and sized before
  it is used; nothing unverified is ever renamed into place.
- A release is installed in a staging folder that becomes the version
  folder in one rename, so an interrupted or cancelled update leaves
  nothing behind. Free space is checked first.

The private key stays on the release host and is never in this repository.
Keep a copy offline: without it no installed client accepts another update.
To rotate, add the new public key to `keys.js`, ship a release signed with
the old key, then switch.

## Recovery

- A version that dies before its window comes up is counted. The hand-off
  then tries the next newest version, down to the installed package. After
  two such failures a version is set aside, and the feed is not asked for
  it again; a newer release clears the way.
- Until its window is up, an error in the main process ends the run with a
  logged reason instead of Electron's error dialog, so whatever started it
  can carry on.
- Collection keeps the running version, newer ones waiting for their first
  launch, the newest healthy version older than the running one, and any
  version a hosted server runs from. Everything else in `versions/` goes.
- `BFE_NO_UPDATE=1` starts the executable as it is, without hand-off or
  check. Deleting the `versions` folder returns to the installed package.

## Game data

A release's channel file names its content generations
(`tools/content-version.json`). When they differ from the running
version's, the loader says the game data is refreshed after the update.
The new version's normal startup check (`electron/prepare.js:
preparedFor`) then prepares again, and `tools/prepare.py` reruns only the
steps the changed generations invalidate. Everything else is kept.

## Release pipeline

`.github/workflows/release.yml` builds the packages and update payloads and
attaches them to a GitHub release tagged `preview-N`; the project's release
host then signs and publishes that release. The scripts involved:

- Every packaged build records its own files in `resources/bfe-tree.json`
  (`build/tree-index.cjs`, after packing and again after signing).
- `build/update-payload.mjs` turns each unpacked app (`linux-unpacked`,
  `linux-arm64-unpacked`, `win-unpacked`) into `update-<platform>.tar`:
  the manifest plus the files the store does not hold yet. The store's
  `blobs.json` lists what it holds. Files are zstd level 19, told their
  size; files over 32 MB use level 12. The first payload of a platform
  carries everything (about 250 MB, 1.5 minutes on an 8-core machine);
  later ones carry only what changed. zstd's own worker threads are not
  used: Node 22's asynchronous zstd writes empty frames with them, and
  Electron's Node refuses them.
- `build/check-update.mjs` has the packaged runtime of a platform, Windows
  included, install its own build from its payload, check every file and
  run the installed copy.
- `build/publish-distribution.py` checks every new blob against its name,
  adds it to the store and writes each platform's manifest into the
  release. After the release is live, it signs and replaces the channel
  file atomically, then deletes blobs no retained release lists.
- `release.yml` sets `bfeUpdateChannel` (`development-preview`) in the
  packaged `package.json`. Local builds have no channel and never update
  themselves or replace an installation.

## Measured

On Ubuntu 24.04 under a userns profile like the Debian package's, an update
between two packaged Linux builds one version apart reused 6,444 of 6,446
files and downloaded 2.2 MB; it installed in 11.5 s (mostly the first copy out
of the system package's folder, later updates hard-link), and the next launch
of the installed package handed off to it within 10 s, sandboxed under the
inherited profile.
