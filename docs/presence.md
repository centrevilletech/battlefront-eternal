# Steam and Discord presence

The desktop client (`bfe-client`, through `rust/bfe-desktop/rust/presence/` and
`rust/bfe-desktop/rust/steam/`) tells the player's friends what they are playing:

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
  began. It is off until the player says yes: the main menu asks once
  ([What friends see](#what-friends-see)).

Neither needs an account with us, a network service of ours, or a third-party
SDK. Both are optional and never stand in the way of starting the game.
Locally generated identities stay the only identity the game uses
([MULTIPLAYER.md](../MULTIPLAYER.md)): nothing here links a Steam or Discord
account.

## Modules

| Module | What it does |
| --- | --- |
| `rust/bfe-desktop/rust/presence/discord.rs` | Discord's local IPC protocol: finding the client's socket, the handshake, `SET_ACTIVITY`, reconnecting |
| `rust/bfe-desktop/rust/presence/activity.rs` | What friends see, from the active screen, map catalog, server and battle state |
| `rust/bfe-desktop/rust/presence/mod.rs` | Presence controller state, rate limiting, and the detail setting (`off`, `game`, `full`) |
| `rust/bfe-desktop/rust/steam/vdf.rs` | Steam's binary KeyValues (`shortcuts.vdf`) and text KeyValues (`loginusers.vdf`, `registry.vdf`) |
| `rust/bfe-desktop/rust/steam/shortcut.rs` | The library entry: Steam's app id scheme, adding, updating and removing it |
| `rust/bfe-desktop/rust/steam/client.rs` | Steam's installations, accounts, the running client, the `steam` command |
| `rust/bfe-desktop/rust/steam/launcher.rs` | What the entry runs on each platform and installation |
| `rust/bfe-desktop/rust/steam/mod.rs` | Adding and removing with Steam closed, artwork, keeping the entry current, and relaunching through Steam |
| `rust/bfe-shell` | OPTIONS > STEAM & DISCORD, and the main menu's one-time questions: Discord activity, and adding the game to Steam |
| `resources/assets/steam/` | The library's artwork (original, no retail art) |

## Steam

### The library entry

Steam lets any program be added to its library as a non-Steam game
(`userdata/<account>/config/shortcuts.vdf`). Started from there, Steam treats
it as a game: friends see the entry's name, play time is counted, and the
library shows its artwork.

The initial time the main menu opens with Steam installed and the game not in
its library, the client asks once (`offerSteam`, after the Discord question):
ADD TO STEAM or NOT NOW.
OPTIONS > STEAM & DISCORD adds or removes it later. The entry:

- is named **Battlefront Eternal**;
- runs the same start the desktop launcher does, with `--steam` as its launch
  options:

  | Installation | The entry runs |
  | --- | --- |
  | Windows installer | the installed `bfe-client.exe` |
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
- gets our capsule, portrait, hero, logo (with its position,
  `<id>.json`, without which Steam does not show a custom logo) and icon in
  `config/grid`, unless the player already set their own for that place.
  Removing the entry removes only pictures still exactly as written.

Steam keeps its library in memory and writes `shortcuts.vdf` back whenever
it changes, so the file is only written while Steam is closed. With Steam
open, adding or removing closes Steam (`steam -shutdown`), writes, and starts
it again (`steam -silent`), after the player agrees to that. Every write is a
complete file renamed into place; the initial time, the previous file is kept
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
terminal) goes through Steam (`rust/bfe-desktop/rust/steam/`):

1. This copy leaves a request with a token in the update folder's
   `launches/` and asks Steam for `steam://rungameid/<id>`.
2. Steam starts the entry. That copy has `--steam` (and Steam's
   `SteamGameId` in its environment), finds the request and leaves the token.
3. This copy exits.

If Steam has not started the game within 20 seconds (signed out, an update
dialog, a broken entry), this copy carries on by itself, and so do the
versions it hands off to (`BFE_NO_STEAM`) and every start of the next half
hour (`launches/steam-relaunch-failed`); nothing about Steam can stop the
game from starting. A second click on the launcher during the round trip
exits and leaves it to the existing process; a request left by a start that died
is ignored. Already running, the game comes to the front without a round trip.
OPTIONS > STEAM & DISCORD > START THROUGH STEAM turns this off;
`BFE_NO_STEAM=1` skips it for one start.

Steam watches the process it started. The installed executable hands off to
the newest update; Steam on Windows would see the game end with it (a
launcher that exits loses "In-Game"), and so would a Flatpak Steam, which
follows `flatpak-spawn`, which ends with the process it started. There,
started by Steam, the hand-off and the restart into a fresh update stay,
windowless, until the game exits, and pass on its exit code. A native Linux
Steam starts games under its reaper, a child subreaper that follows every
process the game starts, so nothing stays there.

A server hosted from the game keeps running after it when the player
chooses so. Under a native Linux Steam's reaper that would keep the game
"playing" in Steam, and Steam's STOP would end the server, so a game Steam
started there starts its server through the user's systemd
(`systemd-run --user`), outside the game's processes; without a user systemd it
starts as usual.

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
  app asks in a dialog of its own; agreeing runs
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
2. Under **General Information**, upload the application icon (`icon.png`),
   and copy the **Application ID**.
3. Under **Rich Presence > Art Assets**, upload `icon.png` with the key `logo`
   (assets take about 10 minutes to appear).
4. Set `DISCORD_APPLICATION_ID` and `DISCORD_LARGE_IMAGE = "logo"` in
   `rust/bfe-desktop/rust/presence/`.

The application id is public (it is sent in the clear to the local Discord
client) and Rich Presence needs no secret, OAuth or approval. Never put the
application's client secret or a bot token in the client. Until an id is set,
the client makes no Discord connection; `BFE_DISCORD_APP_ID` overrides it for
testing.

### The connection

`rust/bfe-desktop/rust/presence/discord.rs` speaks Discord's local IPC protocol:
frames of an opcode and a length (32-bit little-endian) and JSON; a handshake
(`{v: 1, client_id}`) answered with READY; then `SET_ACTIVITY` with the
process's pid. It looks for the client:

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

### What friends see

Nothing goes to Discord until the player chooses: the client does not even
connect to it. The initial time the main menu opens, it asks once
(`offerDiscord`): "Show Discord friends what you play?", with MAP AND SERVER,
GAME ONLY or NO THANKS, and says the answer can be changed any time in
Options, Steam & Discord. Closing the question counts as no. Any answer is
kept (`presence.discord` in `desktop-settings.json`), so it is never asked
again; installs from before the question have no answer yet and are asked
too.

`DISCORD ACTIVITY` in OPTIONS > STEAM & DISCORD changes it: MAP AND SERVER,
GAME ONLY ("Playing Battlefront Eternal" and the time), or OFF (the
default until the player answers). Discord's own "Share my activity" setting
applies on top.

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

## Limits

- **Steam's pop-up.** What friends see in the friends list and on the
  profile is the entry's name. Steam's "is now playing" pop-up is raised when
  a friend's app id changes, and non-Steam games all have app id 0, so it most
  likely stays quiet. Only a Steam app of Battlefront Eternal's own would give
  the pop-up and Steam Rich Presence; Steamworks with the original game's app id
  would show "STAR WARS Battlefront (Classic, 2004)" instead, so it is not used.
- **Starting outside Steam** with Steam closed is not seen by Steam: the game
  does not start Steam for itself.
- **Discord Join** (Ask to Join, party invites) is not used: it needs a launch
  protocol registration and a join path into the server browser, and
  browser-based setups (arRPC) do not carry it.

## Testing

```bash
cargo test --manifest-path rust/Cargo.toml --locked -p bfe-desktop
```
