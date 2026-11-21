# Desktop updates

The desktop client updates itself when it starts on Windows (`x64`), Linux
(`x64` and `arm64`) and macOS (`arm64`): no reinstall, no password, the same
way on every distribution. An update
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
2. **Check** (in the background, a few seconds after start). The client asks
   the release feed of its channel
   (`downloads/updates/development-preview.json`) for the newest release.
   Offline, or with the feed unreachable, nothing changes.
3. **Install.** Files of the new release that any build on this computer
   already has are reused by SHA-256 from `resources/bfe-tree.json` (hard
   links on Linux, copies on Windows); the rest are downloaded from the
   content-addressed blob store (`downloads/updates/blobs/<aa>/<sha256>.zst`),
   decompressed with Zstandard and checked against their SHA-256 and size. On
   a native build where the Python and FFmpeg toolchain has not changed, an
   update downloads only the changed `bfe-client` and `bfeserver` binaries and
   manifest metadata rather than a full installer. The game keeps running;
   the new version is taken at the next start (step 1).
4. **Healthy.** About 10 seconds after its window is up a version counts as
   healthy. Linux launchers are pointed at it (`current`), and versions
   nothing needs are deleted.
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
| macOS | `~/Library/Application Support/Battlefront Eternal/updates` |

Inside: `versions/<version>/` (complete installs, each with its
`.bfe-install.json` record written last), `state.json` (launch attempts,
failed and healthy versions), `launches/` (window-up markers), `current`
(Linux: a link to the newest healthy version) and `update.log`.

Settings, identities, prepared game data and the hosted server's state
stay in the user data folder (`~/.config/Battlefront Eternal`,
`%APPDATA%\Battlefront Eternal`), which no version owns.

Coming from the Electron builds (preview 331 and earlier), whose updater
installs the first native release and hands off to it like any other: the
native client reads their `player-settings.json` and `desktop-settings.json`
as they are, and on its first start copies their player identity
(`network-identity.json`) and online notice consent (`online-consent.json`)
from the user data folder to its own files (`identity.key` and
`online-consent.json` in `${XDG_DATA_HOME:-~/.local/share}/battlefront-eternal`,
`%APPDATA%\battlefront-eternal`), once, leaving the old files in place. The
player keeps the same player ID and is not asked to accept the notice again.

The player's settings (profile name, options, bindings) are kept in
`player-settings.json` and `settings.json` inside the user data folder by
`rust/bfe-desktop/src/settings/`.

### Debian and Ubuntu

The package installs `/opt/Battlefront Eternal`, owned by root, with symlinks
in `/usr/bin/battlefront-eternal`, `/usr/bin/bfe-client` and
`/usr/bin/bfeserver`. Native Linux builds also include a relative
`battlefront-eternal -> bfe-client` symlink inside the version folder so
existing launchers and `.desktop` entries continue to resolve across updates.
`apt upgrade` still works: a newer package simply becomes the newest version.
The Electron packages (preview 331 and earlier) registered
`/usr/bin/battlefront-eternal` with update-alternatives, and their removal
script deletes it while the native package is upgraded over them; the native
package's `postinst` puts the link back.

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

### macOS

The app is `Battlefront Eternal.app`, wherever the player put it. Its programs
are in `Contents/MacOS`, everything else is in `Contents/Resources`, and the
link `Contents/MacOS/resources` lets the programs find it beside themselves
as on the other systems. Nothing is ever written inside the bundle, so its
code signature keeps verifying.

A version in the update folder is a complete copy of the release's app,
`versions/<version>/Battlefront Eternal.app`: every file of the signed bundle
byte for byte, the signature's own files and the notarization ticket
included, so it verifies exactly as the download did. macOS marks every file
of a downloaded app as quarantined; a file reused from the installed app has
that mark removed, and a file the updater fetched never has it, so Gatekeeper
does not question a program the player already accepted. The index inside the
bundle (`Contents/Resources/bfe-tree.json`) lists the files as the programs
see them and leaves out the client and the signature's files, whose hashes
only exist once the bundle is sealed.

macOS asks who signed a program only when a browser downloaded it, never for
files an updater writes. So the updater asks itself
(`rust/bfe-desktop/src/update/signer.rs`): an app signed with a developer
certificate takes an update only when the downloaded bundle verifies as a
whole (`codesign --verify --deep --strict`) and satisfies the requirement the
running app's own signature states for later versions of itself, which for a
Developer ID build is "this bundle identifier, a Developer ID certificate
issued by Apple, this team". Nothing about a team is written in the program;
the installed app is the reference. The bundle that is checked is the
version's own, the one folder at the top of the version: for such an app a
release has to be exactly one app bundle, with its program directly in
`Contents/MacOS` and no file beside the bundle, so the signature that is
checked seals everything the release holds (a program in a bundle nested
inside another one is refused). The check runs after the files are
fetched and before the folder counts as an installed version, and again
each time a start hands off to a version in the update folder (it takes
about a fifth of a second on an Apple silicon Mac), so a folder that
something else wrote into is not started either. A build that fails it is
deleted, written to `update.log` and not fetched again; the running version
stays, and the next release is looked at afresh. An app with
no certificate behind its signature (a developer's own ad hoc build) takes
what its channel offers, as the other systems do. A consequence: if the
project ever signs with another developer team, installed copies refuse
those builds and have to be installed again from the download.

The hand-off starts the new version's program from the update folder. When
that version counts as healthy, it replaces the installed app: a copy is made
beside it, checked in the same way against the installed app's signature,
written to the disk, and exchanged with the installed bundle in one step.
From then on the app in Applications is the current version and starts
without a hand-off. The bundle that was replaced is put aside under a hidden
name beside the app (`.Battlefront Eternal.app.bfe-old`) and removed at a
later start, so a copy of the game started from it at that moment keeps its
files. The installed app stays as it is, and the hand-off keeps taking the
newest version at every start, when its folder is not writable for this
user, when it runs from a read-only place (the disk image, or the copy macOS
makes of a download that was never moved), when the app at that path is not
Battlefront Eternal, or while another copy of it is running. A refusal is
written to `update.log` once per version.

## Trust

Only releases signed with a key listed in `rust/bfe-desktop/src/update/keys.rs`
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
To rotate, add the new public key to `rust/bfe-desktop/src/update/keys.rs`,
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
Only a run on `main` creates a release: the jobs that write to it run in the
repository's `release` environment, which only `main` may use, and pull
requests never start the workflow.
The scripts involved:

- Every packaged build records its own files in `resources/bfe-tree.json`
  (`build/native/stage.py`).
- `build/native/update-payload.py` turns each unpacked app
  (`linux-unpacked`, `linux-arm64-unpacked`, `win-unpacked`, and on macOS the
  folder that holds the signed app bundle)
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
- macOS: the workflow builds the app with an ad-hoc signature and attaches it
  to the release as `macos-arm64-unsigned-build-input.tar.zst`. macOS opens an
  app without a warning only when it is signed with a Developer ID
  certificate and notarized by Apple, and no workflow in this repository
  holds such a certificate. The project's signing Mac takes the archive of a
  release whose commit is on `main` and that this workflow made and uploaded,
  signs and notarizes the app (`build/native/sign-macos.py`), makes the disk
  image (`build/native/package-macos.py`) and `update-darwin-arm64.tar` from
  the stapled app, and hands them to the release host with
  `macos-notarized.json`, a record of their sizes and hashes, and that
  record's signature (`macos-notarized.json.sig`), made with a key that
  exists only on the signing Mac. The release host holds the public key and
  takes a Mac build only when the signature verifies, the record names the
  live release and its commit, and the payload is for the architecture the
  record's disk image names (`build/publish-distribution.py`). It adds the
  Mac build to the live release on the download site, serves the record, its
  signature and the update payload beside the disk image, and signs the
  channel file again with the `darwin-arm64` entry. A release is published
  for Windows and Linux without waiting for this; until the Mac build
  arrives, usually within the hour, the channel has no macOS entry and
  installed Mac clients keep the version they have.
- `.github/workflows/release-mac-files.yml` then attaches the Mac build to
  the GitHub release, every 15 minutes or when started by hand, with its own
  token: `build/ci/release-mac-files.py` takes the four files from the
  download site and checks the record's signature against
  `build/macos-record.pub.pem` (the public half of the signing Mac's key),
  that the record is for that release's tag and commit on `main` and for a
  notarized build, and every file's size and SHA-256 against the record. It
  replaces nothing: a file the release already has is skipped when it is the
  same and fails the run when it differs. Neither the signing Mac nor the
  release host holds a GitHub credential.
- What this does not protect against: whoever controls the release host can
  ship anything to installed clients, because the release host signs the
  update channel; and whoever controls the signing Mac can have any Mac build
  published. Gatekeeper still refuses a Mac app that is not signed and
  notarized, and an installed Mac app takes an update only from the same
  developer's signature (see "macOS" above).
- In the release workflow the Mac job is apart from the release job, so it
  never delays or changes the release of its own run; a Mac job that hangs
  keeps the run open until its timeout (60 minutes, like the other build
  jobs), and the next run starts after that.
- Official release builds embed `resources/bfe-release.json` with channel
  `development-preview`. Local builds without a release channel never update
  themselves or replace an installation.

## Measured

On Ubuntu 24.04, an update between two packaged Linux builds one version apart
reused the unchanged toolchain and asset files by SHA-256 and downloaded only
the changed binaries and manifest metadata; it installed in seconds and the next
launch of the installed package handed off to the new version automatically.
