# Desktop client (Electron)

The desktop client wraps the same engine the browser development build runs.
MULTIPLAYER.md covers the release plan; this note covers what exists today.

## What it does

- `electron/main.js` opens one window on `app://bfe/`. The renderer is
  sandboxed, with context isolation and no Node. Navigation stays on that
  origin, new windows are denied (https links open in the system browser), and
  only pointer lock, fullscreen, keyboard lock and clipboard writes are granted.
- `electron/content.js` serves that origin. Engine files come from the Vite
  build inside the app. `/maps`, `/sides`, `/common` and `/audio` come from the
  player's game data folder, with byte ranges for the audio streams. Every
  response carries a Content-Security-Policy; nothing loads from the network.
  It allows `'unsafe-eval'` because three's KTX2 transcoder (Emscripten
  embind) generates functions at runtime in its decoding workers; without
  it no KTX2 texture ever decodes.
- A battle holds still under its loading screen: no tick runs until every
  requested texture has arrived and been uploaded to the GPU, with shaders
  compiled against them (`game.loaded()`), then the screen closes and
  `game.begin()` starts the world. `game.texturesPending()` lists what the
  load still waits on.
- `electron/preload.cjs` is the only bridge: `window.bfeDesktop` reports the
  platform and the game data folder, opens the folder picker, and gives the
  loader its state and buttons.
- Every battle runs in the world worker, as in a browser.
  `--query=<flags>` opens the main menu with dev flags that launch URLs keep.
- F11 toggles fullscreen. The menu bar is hidden until Alt is pressed; its
  Game menu reopens the main menu or the preparation loader, chooses a game
  or data folder, and opens the data folder or the preparation log.
- Ctrl is crouch, so outside macOS the menu takes no Ctrl+letter shortcut a
  battle uses: Ctrl+W, Ctrl+M, Ctrl+R, Ctrl+Shift+R and Linux's Ctrl+Q do
  nothing, where Electron's stock items would close, minimize, reload or
  quit the game. Exit, Reload, Force Reload and the title bar's buttons
  remain; Alt+F4 still closes the window. macOS keeps its Command shortcuts.

## Updates

On Windows and Linux the client updates itself at every launch: Electron,
the interface, the engine, the hosting runtime and the tools, without
administrator rights. macOS is not supported yet and does not self-update.
Prepared game data is refreshed only when a release changes what
preparation produces. See [desktop-updates.md](desktop-updates.md).

## Prepared content compatibility

`tools/content-version.json` is shared by the Python preparer and the desktop
startup check. Increase its map, side or common generation when generated gameplay
assets change (common: what every side shares, its clips among them;
stored in `.prepare.json` as `commonVersion`). The packaged client reads the same external resource as Python; the installer
smoke test imports it inside the actual Electron archive.
After the application update check, startup checks the state, map, side and common
versions of every selected, imported, managed or discovered content folder.
Outdated or unfinished content automatically prepares from the installed Steam
copy before the menu opens. For imports, original retail archive hashes must
match before relocating and reusing checkpoints; otherwise preparation uses a
separate managed folder and leaves the import intact. Interrupted work resumes
on the next launch. Failed preparation stays on the loader with a retry action.
Current imports still work without Steam. If an outdated import has no local
source, the loader asks for the original installation or a current content
export rather than opening the menu with incompatible data. Explicit developer
`--content-dir` / `BFE_CONTENT_DIR` and `--dev-url` overrides remain unchecked.
The runtime content fingerprint check is still enforced when joining a server;
startup generation checks do not detect arbitrary edits or corruption.

Updating the executable alone cannot update a dedicated server's game data.
Refresh its prepared data from the same generation as the clients, including
map JSON, prop animation and collision binaries, common and side definitions.
Keep the previous server directory until health and a player join are verified.
Use `node tools/content-report.mjs <data-folder> '?map=nab2&side=rep'` on both
copies to compare the exact runtime fingerprint and identify differing files.
`node tools/content-report.mjs --compare <a> <b>` compares every regular battle
of two prepared folders (or `--battles` output saved as `.json`, taken on the
server with its own engine) and exits 1 when any differs.
Texture quality and operating system do not require different gameplay hashes.

Preparation must give the same bits on every platform, because the server's
data and each player's are prepared apart and compared to the last bit. The
tools therefore never take `sin`, `cos`, `atan2` or `x ** y` from the
platform's C library, whose last bits differ between glibc and the Windows
runtime: `tools/portable_math.py` computes them exactly, and square roots use
`math.sqrt`, which is exact everywhere. Paths the outputs record take each
name's spelling on disk (`tools/zero_addon.py` `child`), not the code's:
Windows finds `SIDE` where the disk says `Side`, and Linux does not.
`tests/test_portable_math.py` fails on a platform math call in a tool whose
output the fingerprint reads.

A tool change that alters any fingerprinted output needs a new generation,
even a small one: the clients prepare again, and so must the server. Before a
server deploy, prepare fresh and in full from the owned install with the
release's engine, and compare it with the staged data (a server's own
updater always prepares fresh data, [server-updates.md](server-updates.md)).
A stage that reuses earlier content keeps whatever an earlier tool wrote.

Map generation 15 adds the sky's flyers: a `sky/` folder beside
`map.json` on the five maps whose battles draw ships (tools/zero_sky.py). It
is presentation only and outside the fingerprint; `map.json` and every
fingerprinted file come out byte for byte as at 14 (`content-report.mjs
--compare` of the two: all 30 battles match). So clients and servers at 14
and 15 still play together; a client prepares its maps again once on
update (every map step, then the catalog, environment and texture steps
after them, as for any map generation) and draws the ships only after that.
A server needs nothing new; its updater prepares a fresh data folder for the
release anyway, as it does whenever the tools change
([server-updates.md](server-updates.md)).

Map generation 16 adds the Enhanced sky's flyers:
`sky/flyers.json` also lists each battle's era classes (`enhanced`, with
each class's `era`), and `sky/` holds their models from the side archives,
so Bespin: Platforms gets a `sky/` folder too
([environment-effects.md](environment-effects.md), "Enhanced"). It is
presentation only and outside the fingerprint; `map.json` and every
fingerprinted file come out byte for byte as at 15 (`content-report.mjs
--compare`: all 30 battles match), so clients and servers at 14, 15 and 16
play together. A client prepares its maps again once on update and draws
the Enhanced sky's ships after that; a server needs nothing new.

## Moving prepared content to another computer

Choose **EXPORT CONTENT** on the main menu to save one `.bfepack` file.
It includes prepared maps, upscaled textures and movies, enhanced audio,
SHA-256 checksums, and the unchanged preparation checkpoint/version records.
It excludes the engine, player identity, settings, saves and preparation locks.
Shared audio remains deduplicated; identical filename case variants work on
Windows as well as Linux. Export is unavailable while preparation runs.

On the other computer, install the desktop client and choose **IMPORT CONTENT
EXPORT** on first launch, or **IMPORT CONTENT** on the main menu. Select the
file, wait for verification, then choose Main Menu. That computer needs neither
a Steam directory nor the extraction/upscaling toolchain. Imports stage in a
new folder and become active only after every checksum passes. Cancellation or
a damaged archive leaves the previous content selected. Allow about 7 GB free
for a stock prepared collection, plus space for the downloaded archive.

For later world updates, prepare and export a refreshed collection on the
computer with the original game. An export also records original `.lvl` hashes
when the source is available. If an imported collection is later attached to a
matching Steam installation through Choose Game Folder, those hashes verify
the source and relocate its preparation checkpoint. Current pipeline versions
still decide which stages must rerun. A changed source falls back to normal
preparation. Re-exporting on a computer without Steam retains the provenance.

For scripted transfers, `node tools/content-archive.mjs export <content> <file.bfepack>`
and `node tools/content-archive.mjs import <file.bfepack> <parent>` use the same
archive format and verification as the desktop menus.

Online browsing and public hosting share an accessible, scrollable privacy
dialog. Buttons occupy their own row; keyboard input stays in the modal.
Accept waits for an atomic local save, prevents duplicate submissions, and
shows a retryable error on failure. A failed save never enables online access.

## First launch: finding and preparing the game

Retail data is never packaged. On launch the app shows its loader
(`electron/ui/`) and decides, in order (`electron/main.js`):

1. `--content-dir=<folder>` or `BFE_CONTENT_DIR`, or a data folder the player
   chose: used as it is.
2. The Steam copy of Star Wars: Battlefront (2004), app 1058020, found by
   `electron/discover.js`: the platform's Steam roots (the registry on
   Windows; native, Flatpak and Snap installs on Linux), every library in
   `libraryfolders.vdf` (`steamapps/` and `config/`), the app manifest's
   install folder, then any `steamapps/common` folder laid out like the game.
   When Steam's records name no library holding it, every drive is looked at
   where libraries usually are (`SteamLibrary`, `Steam`, `Games\Steam` and
   the like), each look bounded so an absent network drive cannot stall
   startup: a library on another drive is found either way. When the app's own
   `game-data` folder (in its user data folder) was completely prepared from
   that copy, with the add-ons it has now, the main menu opens; otherwise
   preparation starts.
3. Without a Steam copy, the GOG copy (`findGogInstall`), prepared the same
   way: it has the Steam copy's layout (`GameData/Battlefront.exe`,
   `GameData/Data/_LVL_PC`). Found where GOG's records say: each game's key
   under `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games` (GOG Galaxy and the
   offline installers write one, with its `gameName` and `path`), and Heroic
   Games Launcher's `gog_store/installed.json` (native and Flatpak on Linux,
   macOS, Windows). Then a folder whose name says Battlefront where GOG games
   usually go: `GOG Games` and `GOG Galaxy\Games` on each Windows drive,
   `~/Games/Heroic`, and those Windows folders inside Wine prefixes
   (`~/.wine`, Lutris's in `~/Games`, CrossOver's bottles). Steam's copy wins
   when there are both, as the project's reference edition.
4. With no copy to prepare from: the app's `game-data` folder if it has data,
   then extracted data elsewhere (a checkout's `public/`, a `game-data`
   folder beside the executable or AppImage, `battlefront-eternal/public` in
   the usual code folders, then a bounded home folder search).
5. Nothing: the loader offers IMPORT CONTENT EXPORT, CHOOSE GAME FOLDER,
   GET IT ON STEAM, GET IT ON GOG and SEARCH AGAIN. Import accepts a
   `.bfepack` export; the folder picker accepts a game installation from any
   store or disc (the folder holding `GameData`, or `GameData` itself) or
   prepared data. The installation chosen is kept (`gameDir` in
   `desktop-settings.json`) and looked at before Steam's and GOG's.

A GOG copy (game 1668107107, version 1.3.7.4) matches the Steam copy: 164 of
its 166 `GameData` files are byte for byte the Steam copy's, spelled the same,
among them every `.lvl`, Jabba's Palace (`AddOn/Tat3`) and the movies. On
Windows the registry finds a GOG Galaxy install, whatever drive its library is
on, in about 30 ms. GOG has no `steam_api.dll`, and its `Battlefront.exe` is its
own build, which keeps the foliage random table (the same 1024 bytes) at another
position: `tools/zero_foliage.py` finds the table by its bytes, so every map
with painted foliage prepares from either copy.

Preparing both copies (`prepare.py --core-only`, the same tools and Python)
gives the same fingerprint for all 30 battles (`content-report.mjs
--compare`), so a GOG player joins servers prepared from Steam. The prepared
files are identical apart from `.prepare.json`, which records the game folder,
and six menu music `.opus` files, which differ only in the Ogg stream serial
number ffmpeg picks at random on every encode (and the page CRCs over it), as
two Steam preparations would.

On Windows, `tools/enhance_audio.py` renames a sample's record over the shared
`audio/enhanced/samples/<id>.json`, which Windows refuses while another
battle's run has that file open. A refused rename is retried briefly and
counted as done when a current record is already there, and a battle reuses the
samples an earlier battle finished instead of remaking and rewriting them.

Preparation runs `tools/prepare.py` with the bundled toolchain
(`electron/prepare.js`), into the app's `game-data` folder:

| Step (key) | Tool | After | Needs |
| --- | --- | --- | --- |
| Galaxy: the map catalog and loading pictures (`catalog`) | `zero_shell.py --catalog` | | |
| Each army and local force's units (`side:<side>`) | `extract.py --only side` | the catalog | |
| What every side shares: clips, effects, HUD art (`common`) | `extract.py --only common` | every side | |
| Interface: menus, music and sounds (`shell`) | `zero_shell.py` | common | ffmpeg |
| Each map with all its battles' sides (`map:<map>`) | `extract.py <map> --side ... --skip-shared` | the catalog | |
| The catalog again, marking the maps ready (`catalog-update`) | `zero_shell.py --catalog` | every map | |
| Theed's fountains, Cloud City's steam (`environment`) | `zero_environment.py` | those maps | |
| HUD and font atlases at 4x (`hud`) | `upscale_hud.py` | the shell | Pillow |
| Enhanced sound per battle (`audio:<sublevel>`) | `enhance_audio.py` | its map, and the era's first | numpy, scipy, soundfile, soxr, ffmpeg |
| KTX2 textures (`textures`) | `ktx2.py` | everything that writes textures | KTX-Software `ktx` |
| Movies, upscaled to WebM (`movies`) | `zero_shell.py --movies` | the catalog | ffmpeg with Bink, VP9 and Opus |

Steps run as soon as what they are after is done and the cores allow: the
sides and maps several at once (one process each, as many as memory
allows at about 0.6 GB each), a battle's sound enhancement beside the maps
still extracting (each with half the cores), the movies from the start.
Shared work is done once: a side's units, what every side shares, and
an era's voice-over and the sample bank in the Enhanced sound
(`audio/enhanced/streams/` and `samples/`, hard-linked into each
sub-level's folder). A map is one step whatever its eras; the map's
mission, vehicles and AI mission follow its last listed side, as before.

Add-on maps are prepared like the stock ones. An add-on is a folder in
`GameData/AddOn` with an `addme.script`, as the Steam copy installs Jabba's
Palace in `AddOn/Tat3` (docs/formats.md, Add-ons). `tools/zero_addon.py`
finds every one. The catalog lists its map after the game's, and its map,
its own armies (the Gamorreans) and its battles' sound become steps like
any other (`map:tat3`, `side:gam`, `audio:tat3cw`, `audio:tat3gcw`).
`.prepare.json` records the add-ons a run read (`addOns`). When the
installed ones differ, the app prepares again. That happens when an add-on
is installed or removed, and when the folder was prepared before add-ons
were read, which is how Jabba's Palace went missing. Only what lists or
shares their content runs again: the catalog, `common` (their armies'
clips and their maps' effects), the movies (their flyovers) and
`catalog-update`. Their new steps run beside these, and everything else is
kept. For Jabba's Palace that run took 34 s with four jobs on one development
machine.

The loader stays up until every step is done: the game and preparation
compete badly for CPU and memory, so there is no playing early. Since nothing
else runs meanwhile, the tools get every core (`--jobs` is the core count) at
normal priority; the pipeline sizes what runs side by side (armies, map
extractions, Enhanced sound jobs, movie encodes) to the memory available,
and movies use one VP9 pass (2.8x faster than the two passes measured
before, same quality). Finished steps are recorded in
`game-data/.prepare.json`, so a run stopped by quitting resumes on the next
launch; quitting stops the whole tool process tree. A step whose tool is
missing is skipped and shown in amber. The loader shows the hologram rooms,
then the game's own loading pictures for the map being prepared once the
catalog is in. The log is in the app's user data folder,
`logs/prepare.log` (Game → Open Preparation Log).

The bar and the time left are by expected time, not step count
(`electron/prepare.js`: `estimate`): each step kind has a measured duration
(a side 1 to 17 s, common 10 to 35 s, a map 3 to 30 s with six running side
by side, a sub-level's sound enhancement 25 to 60 s with four jobs, the
textures a few seconds (the extraction steps compress their own as they
go), the movies about 10 minutes with four encodes, which run from the
start under everything else, on an 8-core laptop), replaced by this machine's own measurements as steps finish
(`prepare-timings.json` in the user data folder keeps the last eight of
each kind). Steps that run side by side are divided by how many do, and
once three steps have finished the remainder is scaled by how they compared
with their expectations. The readout shows elapsed and about how long is
left.

The extractor reads an installation in place with `BFE_GAME_DIR` (the folder
holding `GameData`) and writes to `BFE_CONTENT_DIR` instead of `public/`
(`tools/zero_paths.py`). From a checkout:

```bash
BFE_GAME_DIR="$HOME/.local/share/Steam/steamapps/common/Star Wars Battlefront (Classic 2004)" \
  python3 tools/prepare.py
```

### The toolchain

`node build/fetch-toolchain.mjs [linux|mac|win] [x64|arm64 ...]` fills
`toolchain/<os>-<arch>/` (gitignored), which electron-builder packs as
`resources/toolchain` beside `resources/tools`:

- CPython 3.13 from python-build-standalone, with Pillow, numpy, scipy,
  soundfile and soxr installed for the target by pip, test suites pruned;
- a static ffmpeg (BtbN's GPL builds for Linux and Windows,
  martin-riedl.de's for macOS);
- KTX-Software's `ktx` and its library.

Without a toolchain (a plain checkout), the system `python3` runs the tools
and steps whose dependencies are missing are skipped.

## Linux desktop integration

Run from the AppImage, the app registers itself on every launch: a
`battlefront-eternal.desktop` entry named Battlefront Eternal (app
launchers, Omarchy's included, list it; search "battlefront"), its icon, and
`~/.local/bin/battlefront` to start it from a terminal. Moving the AppImage
updates them on the next launch. A `battlefront` command the app did not
create is left alone. `BFE_NO_DESKTOP_INTEGRATION=1` turns this off. The
launchers start the newest installed update directly when there is one
(desktop-updates.md).

## Steam and Discord

Friends on Steam and Discord see the player in Battlefront Eternal. The
client offers once to add itself to the player's Steam library as a non-Steam
game, and then starts through Steam whenever Steam is open, so Steam shows
"Playing Battlefront Eternal" and counts the time. Discord Rich Presence
shows the menus or the battle (map, era, side, server, players) over
Discord's local socket. OPTIONS > STEAM & DISCORD has both. See
[presence.md](presence.md).

## Running and packaging

```bash
npm run desktop        # desktop build, then Electron on it
npm run dev            # in one terminal, then:
npm run desktop:dev    # Electron on the Vite dev server (hot reload)
npm run dist           # package for this platform into release/
npm run dist:dir       # unpacked app only, quicker to inspect
```

Electron must not inherit `ELECTRON_RUN_AS_NODE=1` (some Electron-based
terminals and editors set it); with it, Electron runs as plain Node.

`npm run build:desktop` (`vite build --mode desktop`) does not copy `public/`
into `dist/`, since `public/` holds the extractor's retail-derived output.
`build/check-dist.cjs` refuses to package a `dist/` that contains `maps`,
`sides`, `common` or `audio`. Use `build:desktop`, not `build`, before
packaging. Vite bundles three and three-mesh-bvh, so no `node_modules` ship.

Targets (`electron-builder.yml`):

| Platform | Output | Architectures |
| --- | --- | --- |
| Linux (Ubuntu, Omarchy) | AppImage and deb | x64, arm64 |
| Windows | NSIS installer | x64 |
| macOS (experimental, not supported yet) | dmg and zip | x64, arm64 |

## Continuous integration

`.github/workflows/ci.yml` runs on GitHub-hosted runners for every push to
main, every pull request and on demand: the unit tests (`npm test`, which
need no game files) and a code-only browser bundle. No job in this repository
uses a self-hosted runner.

`.github/workflows/desktop-native.yml` builds an experimental, unsigned macOS
package on demand and uploads it as a workflow artifact. macOS is not a
supported platform yet. The package is ad-hoc signed and needs right-click →
Open (or `xattr -d com.apple.quarantine`) on first launch. The workflow signs
only when the repository defines the secrets `MAC_CSC_LINK` and
`MAC_CSC_KEY_PASSWORD` (a Developer ID Application certificate), and
notarizes only when it also defines `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`
and `APPLE_TEAM_ID`.

`.github/workflows/release.yml` builds the official Windows and Linux preview
packages on GitHub-hosted runners for every push to main that changes more
than documentation, and attaches them to a GitHub release tagged `preview-N`.
The project's release host then publishes that release to
get.battlefronteternal.com with `build/publish-distribution.py`, which signs
the APT repository and the update channel
([desktop-updates.md](desktop-updates.md)); the signing keys never leave that
host. A Windows installer without a code-signing certificate shows a
SmartScreen warning.

## Network process

Online play runs in a separate Electron utility process
(`electron/net-process.js`, started on demand by `electron/net-host.js`). It
owns the UDP sockets and the player's identity key and runs the engine's
native UDP transport (`src/net/`). Each battle gets a fresh MessageChannel:
one port in the utility process, the other delivered to the page by the
preload script as a `{type: "bfe:net-port"}` window message, because ports
cannot cross the context bridge. Game packets never pass through the main
process or wait behind the render thread, and the renderer cannot ask for any
identity signature. The main process keeps master requests, consent and
rendezvous signaling (`electron/network.js`). A crashed utility process is
started again for the next battle.

### Slow links

Node gives each of a host's addresses 250 ms to connect before it moves to
the next ("happy eyeballs"). Over a congested link (round trips of 500 to
800 ms were seen) the IPv4 attempt was dropped before it could
answer, and with no IPv6 route every master request failed with "fetch
failed" (ETIMEDOUT) while curl and browsers got through. `electron/
slow-links.js` gives each attempt 2.5 s: it is the first import of the main
process (`boot.js`) and of the network process, and a hosted server gets the
same as its first argument (`--network-family-autoselection-attempt-
timeout`). A master read that still never gets an answer is tried once more,
signed afresh, then reported as "Can't reach the online service (code)";
requests that change something are never sent twice.

## Not done yet

- The headphone decoder (`tools/make_binaural.py`) is not part of
  preparation: it downloads its HRIR set and needs heavier dependencies.
- The planned server-data import/export flow.
- Final application art (`electron/assets/icon.svg` is a temporary design),
  signing certificates, and Windows arm64.
- Native TURN over TCP/TLS in the network process, for networks that block
  UDP (`docs/native-udp-transport.md`).
