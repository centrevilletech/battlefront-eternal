# Desktop updates

The desktop client updates itself when it starts on Windows (`x64`) and Linux
(`x64` and `arm64`): no reinstall, no password, the same way on every
distribution. macOS is not supported yet and does not self-update. An update
covers everything in the application tree: `bfe-client`, `bfeserver`, the
preparation scripts in `resources/tools/`, the standalone Python and FFmpeg
toolchain in `resources/toolchain/`, legal notices in `resources/legal/` and
icons in `resources/assets/`. Prepared game data is never part of an update; it
is refreshed only when a release changes what preparation produces.

## On every launch

1. **Hand-off** (before the main window opens). If the user's update folder
   holds a healthy version newer than the executable that started, that
   version starts with the same arguments and this one exits once its window
   is up. The installed package stays as it is (the Debian package, the
   AppImage or the Windows installation) and serves as the fallback.
2. **Check** (under the loader). The client asks the release feed of its
   channel (`downloads/updates/development-preview.json`) for the newest
   release. Offline, or with the feed unreachable, it starts as it is after at
   most 5 seconds (a refused connection fails at once).
3. **Install.** Files of the new release that any build on this computer
   already has are reused by SHA-256 from `resources/bfe-tree.json` (hard
   links on Linux, copies on Windows); the rest are downloaded from the
   content-addressed blob store (`downloads/updates/blobs/<aa>/<sha256>.zst`),
   decompressed with Zstandard and checked against their SHA-256 and size. On
   a native build where the Python and FFmpeg toolchain has not changed, an
   update downloads only the changed `bfe-client` and `bfeserver` binaries and
   manifest metadata rather than a full installer. The loader shows the
   progress and a SKIP FOR NOW button. Retried downloads failing three times
   show TRY AGAIN and PLAY WITHOUT UPDATING.
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
`player-settings.json` and `settings.json` inside the user data folder by
`rust/bfe-desktop/rust/settings/`.

### Debian and Ubuntu

The package installs `/opt/Battlefront Eternal`, owned by root, with symlinks
in `/usr/bin/battlefront-eternal`, `/usr/bin/bfe-client` and
`/usr/bin/bfeserver`. Native Linux builds also include a relative
`battlefront-eternal -> bfe-client` symlink inside the version folder so
existing launchers and `.desktop` entries continue to resolve across updates.
`apt upgrade` still works: a newer package simply becomes the newest version.

### AppImage

The Linux one-command installer, `install.sh`, lives in the website
repository, not in this one. It writes `~/.local/bin/battlefront-eternal`,
which ran the AppImage unpacked on every start (for FUSE 3-only systems such
as Omarchy). The app rewrites that launcher, and its own `battlefront`, to
start `current` directly when there is one, and the AppImage otherwise.
A version started from an AppImage gets none of the AppImage mount's paths
(`PATH`, `LD_LIBRARY_PATH` and the rest disappear with the mount); the
AppImage stays known as `BFE_APPIMAGE`.

### Windows

The NSIS installer is per user (`%LOCALAPPDATA%\Programs\Battlefront Eternal`);
updates go beside it in `%LOCALAPPDATA%\Battlefront Eternal`, which needs no
administrator rights. Uninstalling removes `versions`, `launches`,
`state.json` and `update.log` (`build/native/installer.nsi`,
`build/installer.nsh`) unless invoked with `/updated`, and keeps the player's
settings, saves and prepared game data in `%APPDATA%\Battlefront Eternal`.

## Trust

Only releases signed with a key listed in `rust/bfe-desktop/rust/update/keys.rs`
are installed. Choosing another master server or feed grants no code
execution.

- The channel file `downloads/updates/<channel>.json` is an envelope: the
  release JSON (version, release id, content generations and, per
  platform, the manifest's path, SHA-256, `executable`, optional
  `server_executable`, file count and total size), its Ed25519 signature and
  the key id (the initial 16 hex digits of the SHA-256 of the public key's
  SubjectPublicKeyInfo).
- Each platform's manifest (`downloads/<release>/update-<platform>.json.zst`)
  lists every file with its size, SHA-256, compressed size and mode, plus
  links and empty folders. Both `executable` and `server_executable` (when
  present) must be relative paths inside the version folder with mode `0755`.
- Files come from a content-addressed store,
  `downloads/updates/blobs/<aa>/<sha256>.zst`, named by the SHA-256 of
  their contents. Each download is decompressed, hashed and sized before
  it is used; nothing unverified is ever renamed into place.
- A release is installed in a staging folder that becomes the version
  folder in one rename, so an interrupted or cancelled update leaves
  nothing behind. Free space is checked first.

The private key stays on the release host and is never in this repository.
Keep a copy offline: without it no installed client accepts another update.
To rotate, add the new public key to `rust/bfe-desktop/rust/update/keys.rs`,
ship a release signed with the old key, then switch.

## Recovery

- A version that dies before its window comes up is counted. The hand-off
  then tries the next newest version, down to the installed package. After
  two such failures a version is set aside, and the feed is not asked for
  it again; a newer release clears the way.
- Until its window is up, an error during startup ends the run with a
  logged reason instead of a blocking modal dialog, so whatever started it
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
The new version's normal startup check then prepares again, and
`tools/prepare.py` reruns only the steps the changed generations invalidate.
Everything else is kept.

## Release pipeline

`.github/workflows/release.yml` builds the packages and update payloads and
attaches the public installers and server archives to a GitHub release tagged
`preview-N`; the project's release host then signs and publishes that release.
The scripts involved:

- Every packaged build records its own files in `resources/bfe-tree.json`
  (`build/native/stage.py`).
- `build/native/update-payload.py` turns each unpacked app
  (`linux-unpacked`, `linux-arm64-unpacked`, `win-unpacked`)
  into `update-<platform>.tar`: `update-<platform>.json` plus the compressed
  blobs the store does not hold yet. The store's `blobs.json` lists what it
  holds (`sha256` to compressed byte size). Files up to 32 MB use Zstandard
  level 19; files over 32 MB use level 12.
- `build/native/check-packages.py` verifies the unpacked tree, installers,
  server archive and update payload on the runner, runs `bfe-client --help`,
  `bfeserver --help` and the bundled Python interpreter, and reports package
  sizes.
- `build/publish-distribution.py` checks every new blob against its name,
  adds it to the store and writes each platform's manifest into the
  release. After the release is live, it signs and replaces the channel
  file atomically, then deletes blobs no retained release lists.
- Official release builds embed `resources/bfe-release.json` with channel
  `development-preview`. Local builds without a release channel never update
  themselves or replace an installation.

## Measured

On Ubuntu 24.04, an update between two packaged Linux builds one version apart
reused the unchanged toolchain and asset files by SHA-256 and downloaded only
the changed binaries and manifest metadata; it installed in seconds and the next
launch of the installed package handed off to the new version automatically.
