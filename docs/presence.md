# Steam and Discord presence

The desktop client tells the player's friends what they are playing:

- **Steam** shows friends the player in "Battlefront Eternal" (the friends
  list and the profile; others see "In non-Steam game"), and counts play
  time. Battlefront Eternal has no Steam app of its own, so it joins the
  player's library as a non-Steam game, and starts through Steam whenever
  Steam is open. Steam's "is now playing" pop-up most likely does not fire:
  the client raises it when a friend's app id changes, and every non-Steam
  game has app id 0 ([Limits](#limits)).
- **Discord** shows "Playing Battlefront Eternal" with what the player is
  doing: in the menus, an Instant Action battle (map, era, side), or an online,
  LAN or hosted battle (map, server name, players), with the time since it
  began.

Neither needs an account with us, a network service of ours, or a native
module. Both are optional and never stand in the way of starting the game.
Locally generated identities stay the only identity the game uses
(MULTIPLAYER.md): nothing here links a Steam or Discord account.

## Files

| File | What it does |
| --- | --- |
| `electron/presence/discord.js` | Discord's local IPC protocol: finding the client's socket, the handshake, `SET_ACTIVITY`, reconnecting |
| `electron/presence/activity.js` | What friends see, from the page's URL, the map catalog, the server and the battle's reports (pure) |
| `electron/presence/index.js` | The main process's state: window navigation, server lists, hosting, page reports, the detail setting |
| `electron/presence/application.js` | The Discord application id |
| `src/presence.js` | The pages' reports: the front end's screen, a battle's start, the player's side, players online |
| `electron/steam/vdf.js` | Steam's binary KeyValues (`shortcuts.vdf`) and text KeyValues (`loginusers.vdf`, `registry.vdf`) |
| `electron/steam/shortcut.js` | The library entry: Steam's app id scheme, adding, updating and removing it |
| `electron/steam/client.js` | Steam's installations, accounts, the running client, the `steam` command |
| `electron/steam/launcher.js` | What the entry runs on each platform and installation |
| `electron/steam/index.js` | Adding and removing with Steam closed, artwork, keeping the entry current |
| `electron/steam/relaunch.js` | A start from outside Steam going through it (at boot) |
| `electron/steam/overlay.js` | Where Steam's overlay can work, the switches that let it draw (Windows), whether it loaded |
| `src/shell/integrations.js` | OPTIONS > STEAM & DISCORD, and the one-time offer to add the game to Steam |
| `electron/assets/steam/` | The library's artwork, drawn by `tools/steam-artwork.mjs` (original, no retail art) |

## Steam

### The library entry

Steam lets any program be added to its library as a non-Steam game
(`userdata/<account>/config/shortcuts.vdf`). Started from there, Steam treats
it as a game: friends see the entry's name, play time is counted, and the
library shows its artwork.

The first time the main menu opens with Steam installed and the game not in
its library, the client asks once (`offerSteam`): ADD TO STEAM or NOT NOW.
OPTIONS > STEAM & DISCORD adds or removes it later. The entry:

- is named **Battlefront Eternal**;
- runs the same start the desktop launcher does (`launcher.js`), with
  `--steam` as its launch options:

  | Installation | The entry runs |
  | --- | --- |
  | Windows installer | the installed `Battlefront Eternal.exe` |
  | Debian or Ubuntu package | `/opt/Battlefront Eternal/battlefront-eternal` |
  | AppImage from `install.sh` (Omarchy, Arch and others) | `~/.local/bin/battlefront-eternal` |
  | AppImage run directly | `~/.local/bin/battlefront`, else the AppImage |

  Each of these hands off to the newest installed update
  ([desktop-updates.md](desktop-updates.md)), so the entry never changes
  with an update. If the launcher moves (an AppImage moved, the package
  replaced), the next start with Steam closed points the entry at it,
  keeping launch options the player wrapped around ours
  (`gamemoderun %command% --steam`); an entry the player pointed at a
  program of their own is left alone;
- has Steam's app id for shortcuts: the CRC-32 of its executable and name,
  top bit set, as Steam ROM Manager and BoilR compute it. It is chosen once and
  kept, with the play time and artwork it collects;
- asks for the Steam overlay off (`AllowOverlay` 0), which newer clients
  ignore ([Overlays](#overlays));
- gets our capsule, portrait, hero, logo (with its position,
  `<id>.json`, without which Steam does not show a custom logo) and icon in
  `config/grid`, unless the player already set their own for that place.
  Removing the entry removes only pictures still exactly as written.

Steam keeps its library in memory and writes `shortcuts.vdf` back whenever
it changes, so the file is only written while Steam is closed. With Steam
open, adding or removing closes Steam (`steam -shutdown`), writes, and starts
it again (`steam -silent`), after the player agrees to that. Every write is a
complete file renamed into place; the first time, the previous file is kept
as `shortcuts.vdf.bfe-backup`. Other programs' entries are written back with
exactly the bytes they were read with, and a file that does not parse is
never written. An entry the player deletes in Steam stays deleted.

The entry goes to the account signed in to the running Steam, else to the
most recent account (`loginusers.vdf`; newer clients no longer mark
`MostRecent`, so its `Timestamp` decides). A family computer with several
accounts gets it for that one; each account can add it from the options.
Steam is never closed under a game: not when Steam started this one, not
while it runs another (`RunningAppID`), and not in a gamescope session
(Steam Deck's Game Mode, where Steam is the session); the options say what
to do instead.

### Starting through Steam

Steam only knows a game runs when it started it. So when Steam is open and
signed in to the account whose library has the entry, a start from anywhere
else (the desktop launcher, the Start menu, `battlefront-eternal` in a
terminal) goes through Steam (`relaunch.js`, at the very start of
`electron/boot.js`):

1. This copy leaves a request with a token in the update folder's
   `launches/` and asks Steam for `steam://rungameid/<id>`.
2. Steam starts the entry. That copy has `--steam` (and Steam's
   `SteamGameId` in its environment), finds the request and leaves the token.
3. This copy exits. The round trip took 1.3 to 2 s against a stand-in
   Steam (`tests/desktop-steam-launch.mjs`).

If Steam has not started the game within 20 seconds (signed out, an update
dialog, a broken entry), this copy carries on by itself, and so do the
versions it hands off to (`BFE_NO_STEAM`) and every start of the next half
hour (`launches/steam-relaunch-failed`); nothing about Steam can stop the
game from starting. A second click on the launcher during the round trip
exits and leaves it to the first; a request left by a start that died is
ignored. Already running, the game comes to the front without a round trip.
OPTIONS > STEAM & DISCORD > START THROUGH STEAM turns this off;
`BFE_NO_STEAM=1` skips it for one start.

Steam watches the process it started. The installed executable hands off to
the newest update; Steam on Windows would see the game end with it (a
launcher that exits loses "In-Game"), and so would a Flatpak Steam, which
follows `flatpak-spawn`, which ends with the process it started. There,
started by Steam, the hand-off and the restart into a fresh update stay,
windowless, until the game exits, and pass on its exit code
(`launchVersion({ stay })`, `steamStays`). A native Linux Steam starts games
under its reaper, a child subreaper that follows every process the game
starts, so nothing stays there. A process that stays costs about 120 MB
(about 220 MB when it already showed the loader); the game it started
follows it (`BFE_STEAM_PARENT`), so Steam's STOP ends the game too.

Installed executables from before this change hand off and exit instead
of staying, so on Windows and with a Flatpak Steam, Steam soon stops showing
the game. The game notices (`steamLost`) and OPTIONS > STEAM & DISCORD asks
the player to reinstall once.

A server hosted from the game keeps running after it when the player
chooses so. Under a native Linux Steam's reaper that would keep the game
"playing" in Steam, and Steam's STOP would end the server, so a game Steam
started there starts its server through the user's systemd
(`systemd-run --user`, `host.js: startOutside`), outside the game's
processes; without a user systemd it starts as usual.

On Linux Steam preloads its overlay library into every game it starts
(`LD_PRELOAD=...gameoverlayrenderer.so`), with the overlay turned off too, and
enables its Vulkan overlay layer. In Electron apps that library has crashed
Chromium's sandboxed GPU process and zygote (Electron 37 and 43 reports, one
on Omarchy). Started that way, the game starts itself again at once, in
place (`process.execve`: the same process, so Steam keeps following it),
without the library, the layer and any Steam runtime library path, before
any window or GPU process exists (`withoutSteamOverlay`).
`tests/desktop-steam-launch.mjs` checks that none of the game's processes
maps the library.

### Per platform

- **Windows:** Steam's root and running client come from the registry
  (`HKCU\Software\Valve\Steam`, `ActiveProcess\pid` and `ActiveUser`).
- **Linux, native Steam** (Ubuntu's `steam-installer` package and Valve's
  `.deb` under `~/.steam/debian-installation`, Arch's and Omarchy's `steam`
  under `~/.local/share/Steam`): the running client is found in `/proc` (its
  executable is `ubuntu12_32/steam`), its signed-in account in
  `~/.steam/registry.vdf`, and `steam` in `PATH` talks to it.
- **Flatpak Steam** (`com.valvesoftware.Steam`): it cannot see programs
  outside its sandbox. Its entry asks the host to start the launcher
  (`/usr/bin/flatpak-spawn --host --watch-bus <launcher> --steam`, starting
  in the home folder, which the sandbox has too), which needs Steam to be
  allowed to talk to `org.freedesktop.Flatpak`. The offer says so, and the
  app asks in a dialog of its own (never on a page's word); agreeing runs
  `flatpak override --user --talk-name=org.freedesktop.Flatpak com.valvesoftware.Steam`.
  That permission lets anything Steam starts run programs outside the
  sandbox, which is why it is the player's choice, and removing the game
  from Steam takes it back (`--no-talk-name`).
- **Snap Steam:** its confinement reaches only the home folder, `/media`
  and `/mnt`, not `/opt` where the Ubuntu package installs the game, and
  runs programs against the snap's own libraries. The options say it cannot
  start the game.
- **macOS:** not supported (no desktop updater or Steam entry yet).

## Discord

### The application

Discord shows "Playing <name>" from a Discord application. Battlefront
Eternal's is `1561126739894210652`, named "Battlefront Eternal" with its
icon and the `logo` art asset set (made 2026-10-17). It was made once in the
Discord Developer Portal by the project owner:

1. At <https://discord.com/developers/applications>, choose **New
   Application** and name it **Battlefront Eternal** (the name players'
   friends see). A Team can own it.
2. Under **General Information**, upload `electron/assets/icon.png` as the
   app icon, and copy the **Application ID**.
3. Under **Rich Presence > Art Assets**, upload `electron/assets/icon.png`
   with the key `logo` (assets take about 10 minutes to appear).
4. Put the id in `electron/presence/application.js`
   (`DISCORD_APPLICATION_ID`) and set `DISCORD_LARGE_IMAGE` to `"logo"`.

The application id is public (it is sent in the clear to the local Discord
client) and Rich Presence needs no secret, OAuth or approval. Never put the
application's client secret or a bot token in the client. Until an id is set,
the client makes no Discord connection; `BFE_DISCORD_APP_ID` overrides it for
testing.

### The connection

The main process speaks Discord's local IPC protocol
(`electron/presence/discord.js`): frames of an opcode and a length (32-bit
little-endian) and JSON; a handshake (`{v: 1, client_id}`) answered with
READY; then `SET_ACTIVITY` with the main process's pid. It looks for the
client:

| Client | Where |
| --- | --- |
| Windows | `\\?\pipe\discord-ipc-0` to `9` |
| Discord on Linux (deb, tar, Arch), Vesktop, Legcord, arRPC | `$XDG_RUNTIME_DIR/discord-ipc-N` (else `/run/user/<uid>`, `TMPDIR`, `/tmp`) |
| Flatpak Discord | `app/com.discordapp.Discord/` and `.flatpak/com.discordapp.Discord/xdg-run/` under the runtime folder |
| Snap Discord | `snap.discord/` |
| Flatpak Vesktop, Legcord | `.flatpak/<id>/xdg-run/` |
| macOS | `$TMPDIR/discord-ipc-N` |

Discord need not be running: the client looks again after 5 seconds,
doubling to a minute, while there is something to show. Updates are
coalesced to the newest and sent at most every 4 seconds (Discord allows 5
in 20 seconds). An application id Discord does not know is not tried again.
Clearing (detail OFF) sends an empty `SET_ACTIVITY` and closes once Discord
has answered: while the game's process lives, Discord would otherwise keep
the last activity of a closed socket. Quitting the game clears it too.

The same code was checked against a stand-in that answers as Discord does
(`tests/presence-discord.test.js`, `tests/desktop-presence-electron.mjs`) and
against arRPC 3.6.0, the server Vesktop and browser Discord use.

### What friends see

`DISCORD ACTIVITY` in OPTIONS > STEAM & DISCORD: MAP AND SERVER (the
default), GAME ONLY ("Playing Battlefront Eternal" and the time), or OFF.
Discord's own "Share my activity" setting applies on top.

| Where | Details | State |
| --- | --- | --- |
| Loader | Preparing game data / Updating the game / Starting up | |
| Front end | In the menus | Choosing Instant Action maps / Looking for a server |
| Instant Action | Instant Action: Naboo: Theed | Clone Wars, Republic |
| Online | Online: Tatooine: Mos Eisley | the server's name, (12 of 64) |
| LAN | LAN: Naboo: Theed | the server's name |
| Hosting | Hosting: Naboo: Theed | the server's name, (3 of 8) |

The map names are the catalog's (`shell.json`, from the player's own game);
the server's name and player limit are what the server list or the hosting
dialog showed; the battle reports the player's side and how many players are
in. The elapsed time runs from the battle's start (after loading). The one
piece of free text is the server's name as its operator set it, cut to one
printable line of 64 characters (no control or direction-changing
characters); no address, identity or chat is shown. The party id is a hash
salted per run, not the server's id.

### Omarchy and other browser-only setups

Omarchy installs Discord as a Chromium web app, and a web page cannot offer
Discord's local socket. For presence there, use the native client (`discord`
from Arch's repository), Vesktop, or keep the web app and run arRPC (AUR
`arrpc`, `systemctl --user enable --now arrpc`) with the Vencord Web
extension's arRPC plugin. Flatpak Discord's wrapper forwards only its first
socket; the client looks for it there.

## Overlays

Steam's overlay (Shift+Tab: friends, chat, the web browser) is a library
Steam loads into the process it starts. It draws into each frame that
process presents and reads the keyboard there to see Shift+Tab. Electron
presents frames from a GPU process of its own, so by default the overlay
never sees one.

- **Windows: OPTIONS > STEAM & DISCORD > STEAM OVERLAY**, off until the
  player turns it on (`electron/steam/overlay.js`). Then, when Steam starts
  the game, Chromium's GPU work runs in the game's main process
  (`--in-process-gpu`) and DirectComposition is off
  (`--disable-direct-composition`), as Electron games sold on Steam do; the
  game redraws every frame, which the overlay also needs. It is
  experimental: with Electron 25 and later the overlay has shown as a white
  rectangle until the window is resized
  ([steamworks.js #116](https://github.com/ceifa/steamworks.js/issues/116)),
  and the switches have caused duplicate windows on Alt+Tab
  ([#95](https://github.com/ceifa/steamworks.js/issues/95)). They also give
  up the GPU sandbox and GPU crash recovery (a driver crash closes the game).
  OVERLAY THIS START says whether Steam's library is in the game's process
  (Node's diagnostic report lists the loaded libraries): the game may be the
  version the installed executable handed off to, which Steam only reaches
  if it follows child processes. Nothing else changes: the world worker, the
  network process and audio stay where they are, and starts outside Steam
  keep Electron's usual GPU process.
- **Linux: none.** On Wayland (Hyprland, GNOME, KDE), Steam's overlay
  cannot hook a native Wayland window at all; Valve's Steam Runtime
  maintainer put it as "only works as intended when using X11/GLX, not when
  using X11/EGL or Wayland/EGL"
  ([steam-for-linux #13361](https://github.com/ValveSoftware/steam-for-linux/issues/13361),
  July 2026). Electron stays a native Wayland app there. On X11 it would
  draw (Chromium presents through GLX, about 60 swaps a second in the
  game's process with `--in-process-gpu`), but Valve's current
  `gameoverlayrenderer.so` reads input only through Xlib (`XNextEvent`,
  `XPending`, `XCheck*`), while Chromium reads keys as XInput2 events
  through XCB. A probe that hooks what the overlay hooks saw every key arrive
  through `xcb_poll_for_event` and none through Xlib, so Shift+Tab could
  never open the overlay and it could not hold the game's input while open.
  The game removes the library instead (the crash fix above).
- **Steam Deck Game Mode** (and SteamOS, Bazzite): gamescope draws Steam's
  overlay itself, apart from the game, so it is expected to work there
  (untested).
- **Discord overlay** (Windows only): a separate window that takes the focus
  while open. The game then loses pointer lock and pauses, as for any other
  window, and a click resumes it. Its default keys (Shift+\` or Ctrl+\`) reach
  the game too; with Shift or Ctrl held, \` no longer toggles the stats line
  (`src/input.js`: `overlayChord`).

The library entry still asks for the overlay off (`AllowOverlay` 0): newer
Steam clients keep that choice in their own settings and ignore it, and the
game decides for itself as above.

## Limits

- **Steam's pop-up.** What friends see in the friends list and on the
  profile is the entry's name. Steam's "is now playing" pop-up is raised when
  a friend's app id changes (read from the client's code; not tried on a live
  client), and non-Steam games all have app id 0, so it most likely stays
  quiet. Only a Steam app of Battlefront Eternal's own would give the pop-up
  and Steam Rich Presence; Steamworks with the original game's app id would
  show "STAR WARS Battlefront (Classic, 2004)" instead, so it is not used.
- **Starting outside Steam** with Steam closed is not seen by Steam: the game
  does not start Steam for itself.
- **Steam Input:** for controllers other than Valve's, Chromium sees the
  physical pad and Steam's virtual one. The game reads the first pad, only
  when flying, so the two do not add up.
- **Discord Join** (Ask to Join, party invites) is not used: it needs a launch
  protocol registration and a join path into the server browser, and
  browser-based setups (arRPC) do not carry it.

## Testing

```bash
node --test tests/presence-*.test.js tests/steam-*.test.js
# Electron, a stand-in Discord socket and Steam folders (needs prepared data):
BFE_CONTENT_DIR=/path/to/game-data PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs \
  xvfb-run -a node tests/desktop-presence-electron.mjs
# A packaged build's start through a stand-in Steam (Linux):
BFE_PACKAGED=release/linux-unpacked BFE_CONTENT_DIR=/path/to/game-data \
  xvfb-run -a node tests/desktop-steam-launch.mjs
```

Not tested here: a real Steam client (Windows, Ubuntu, Omarchy, Flatpak,
Steam Deck) and a real Discord client. Before release, add the game to Steam
on each, start it from the desktop launcher, and check a friend sees it.
