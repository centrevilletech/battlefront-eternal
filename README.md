# Battlefront Eternal

A fan-made engine, written in Rust, that plays *Star Wars: Battlefront* (2004)
from your own Steam or GOG copy, with *Star Wars: Battlefront II* (2005)
arriving in the next preview. One OpenGL renderer, one 60 Hz simulation,
Instant Action against bots and online battles on dedicated servers, on
Windows, Linux (x64 and ARM64) and macOS.

> **Battlefront II arrives in the next preview.** Today's download plays
> Battlefront (2004). The next preview opens Battlefront II (2005) for
> testing: 76 of the 2005 game's 89 Instant Action missions, heroes,
> vehicles, Space battles and online play. Weapons, Space flight, sound and
> bots are being tuned against the 2005 game right now, and everything this
> page marks "next preview" is built and in testing, not yet in the download.

You bring the game: no original game files are included in this repository
or distributed with the app. On first launch the app finds your copy and
prepares its data on your computer.

Much of Battlefront Eternal's code and documentation was written with AI
coding assistants, directed and play-tested by Josh Lambert.

Website and downloads: <https://battlefronteternal.com>

## Why OpenGL

Battlefront Eternal draws everything through **one renderer built on
OpenGL**, and it starts there on purpose: OpenGL reaches more computers than
any other choice. These are classic games, and a classic game should play on
classic computers: an old laptop, a living-room PC with integrated graphics,
an ARM64 Linux machine, a Mac, as well as a current gaming card. OpenGL is a
first-class renderer here, not a legacy fallback, and it stays one.

- **It runs on what you have.** The renderer needs OpenGL ES 3.0 or
  OpenGL 3.3, a feature level graphics chips have offered since the early
  2010s. Its measured floor is a 2011 integrated graphics chip, Intel HD
  Graphics 2000, which plays the 2004 maps measured on it at about 40 to 65
  frames a second at 1080p (the table under [What you need](#what-you-need)).
- **No minimum-GPU cliff.** There is no second, reduced game for older
  hardware. Every machine that starts the game gets every map, every mode
  and every graphics option. The four quality presets change how much is
  drawn, never how a battle plays: the simulation and its rules are
  identical at every setting.
- **One code path on every system.** The same shaders (GLSL ES 3.00,
  rewritten to desktop GLSL 3.30 where a driver wants that) run on Windows,
  on Linux under X11 or Wayland, on x64 and ARM64, and on macOS, where the
  system's OpenGL 4.1 core profile is drawn through Metal on Apple silicon.
  A fix or an improvement made once lands on every platform at once.
- **The whole look, tuned on it.** Cascaded shadows, reflective water,
  weather, destruction effects, bloom, upscaling and each game's own Classic
  looks are all built and tuned on this renderer today.

### Planned: a modern rendering path, as an option

**Planned, not built yet.** OpenGL comes first because it reaches the most
computers. The next step is a second, modern rendering path for better
visual effects on current graphics hardware, offered as an option beside
OpenGL: the player or the app picks either one, and the simulation and the
battles are identical on both.

- It will cover Metal on macOS, and Direct3D and Vulkan class hardware on
  Windows and Linux.
- How it is built, as native pipelines or on a modern cross-platform layer
  such as [wgpu](https://github.com/gfx-rs/wgpu), which is designed for
  exactly Vulkan, Metal and Direct3D 12, will be decided by best practice
  once the game parity work for both games is done.
- OpenGL stays a first-class renderer either way, so the game keeps running
  on the widest range of computers.

Dates depend on testing and are not promised.

## What you can play today

Development previews run on Windows 10 and 11 (64-bit), on Linux (x64 and
ARM64) and on macOS (Apple silicon), and update themselves. Today's preview
plays Battlefront (2004). From the next preview the app plays either game or
both, whichever it finds, and a playlist, in Instant Action or on a server,
can mix battles from both.

### Star Wars: Battlefront (2004)

- **Battles.** Instant Action on all 16 original maps plus Jabba's Palace, in
  every era each one offers: 17 maps and 30 battles of Conquest. Bots fight,
  capture command posts, drive and fly by the original game's rules. The
  locals (Tusken Raiders, Jawas, Gungans, Wookiees, Ewoks and the rest) join
  in where the missions place them.
- **Vehicles.** Speeders, tanks, starfighters, AT-ATs, AT-TEs and the other
  command vehicles, with their seats, weapons, mobile spawn points and damage
  effects. Destructible objectives, turrets and map interactives such as the
  Ewok log traps and the snowspeeder's tow cable.
- **Movies.** The game's own menu movies and map flyovers play straight from
  your game folder, decoded by this project's
  [Bink decoder](https://github.com/centrevilletech/bink-decoder); nothing is
  converted when the game is prepared.

### Star Wars: Battlefront II (2005): next preview

Built and in testing; not in today's download. What the next preview opens:

- **24 maps, 76 missions.** Every map of the 2005 game, ground and Space,
  in Conquest, 2-flag CTF, 1-flag CTF, Hunt, XL, Hero Assault and Space
  Assault, by the 2005 game's own mission scripts.
- **Heroes and villains.** Play as a hero in Hero Assault, or earn one in the
  other modes, with lightsaber combos, blocking, deflecting, the thrown
  lightsaber and Force powers ([powers](docs/powers.md),
  [melee](docs/melee.md)).
- **Space battles.** Take off from your capital ship's hangar in a
  starfighter, bomber or gunship, take down the enemy ship's shields and
  systems, or fly into its hangar and fight on board. Bots fly too.
- **Ground vehicles.** The 2005 game's hovers, walkers and ground flyers, with
  their seats, weapons and bot drivers ([vehicles](docs/vehicles.md)).
- **Its own rules and look.** A Battlefront II battle plays by that game's
  rules, with sprint and the forward roll on foot, and shows its own
  controls, loading screen, HUD, spawn screen and scoreboard.

| Map | Clone Wars | Galactic Civil War |
| --- | --- | --- |
| Coruscant | Conquest, 2-flag CTF | Conquest, 2-flag CTF |
| Dagobah | Conquest | Conquest |
| Death Star | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Endor | | Conquest, 1-flag CTF, Hunt |
| Felucia | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Geonosis | Conquest, Hunt, XL | |
| Hoth | | Conquest, 1-flag CTF, XL |
| Jabba's Palace | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Kamino | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Kashyyyk | Conquest, Hunt | Conquest, 2-flag CTF |
| Mos Eisley | Conquest, 2-flag CTF | Conquest, 2-flag CTF, Hunt, Hero Assault |
| Mustafar | Conquest, 2-flag CTF | Conquest |
| Mygeeto | Conquest | Conquest |
| Naboo | Conquest, 2-flag CTF, Hunt | Conquest, 2-flag CTF |
| Polis Massa | | 2-flag CTF |
| Tantive IV | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Utapau | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Yavin 4 | Conquest, 1-flag CTF | Conquest, 1-flag CTF |
| Space Felucia | Space Assault, 1-flag CTF | |
| Space Hoth | | Space Assault, 1-flag CTF |
| Space Kashyyyk | Space Assault, 1-flag CTF | |
| Space Mygeeto | Space Assault, 1-flag CTF | |
| Space Tatooine | | Space Assault, 1-flag CTF |
| Space Yavin | | Space Assault, 1-flag CTF |

Not there yet in Battlefront II, even in the next preview:

- **13 more missions.** The menus will show them as not playable yet: 2-flag CTF
  on Dagobah, Geonosis, Mygeeto, Polis Massa (Clone Wars), Kashyyyk (Clone
  Wars) and Mustafar (Galactic Civil War); Hunt on Hoth; XL on Kashyyyk; and
  Conquest on Polis Massa.
- **Known differences.** Some weapons' effects, sizes and damage, the Space
  turrets and ship damage, the sound mix and the bots on some maps do not
  match the 2005 game yet. They are being measured against it and fixed.
- **Galactic Conquest, the campaign and split screen** are planned.

### In the app

- **Classic and Enhanced.** Each battle can be played the original way or
  with this project's additions (below).
- **Graphics.** Cascaded shadows, reflective water, weather, destruction
  effects, bloom, upscaling and four quality presets (Minimum, Performance,
  Balanced and High Quality). On first launch the app measures the graphics
  card and picks a starting preset.
- **Fixed 60 Hz gameplay.** Every battle, offline or online, runs on a fixed
  60 Hz simulation tick, whatever the frame rate. The picture is drawn on its
  own schedule, at the display's refresh rate or a frame limit from 30 to 240
  frames a second, and the graphics settings never change the rules.
- **Sound.** Classic and Enhanced modes, with 3D audio on headphones and
  speaker layouts up to 7.1.4.
- **Multiplayer.** Server-authoritative battles at 60 Hz over an encrypted
  UDP transport, with bots filling empty slots. An online server browser with
  measured ping (from the next preview it also shows which game each
  server's battles need), LAN games,
  direct connect, and hosting from the app. A server behind a home router is
  reached directly when the routers allow it (hole punching), else through
  the project's relay. No account needed.
- **One launcher (next preview).** Start with only Battlefront (2004), only
  Battlefront II (2005) or both, and add the other game later from the main
  menu.
- **Your copy stays yours.** The app finds the game in Steam, GOG Galaxy or
  Heroic (or a folder you choose), prepares its data in your user folder, and
  only ever reads your installation. Prepared data can be exported and
  imported on another computer.
- **Friends.** Steam can show you as playing Battlefront Eternal, and Discord
  Rich Presence shows the map and server.

## Classic and Enhanced

Battlefront Eternal keeps the original games as the reference and offers its
additions as options, each one CLASSIC or ENHANCED in the menus:

- **Bots.** Classic bots are the original game's AI, quirks included.
  Enhanced bots keep its navigation, weapons, vehicles and squad commands and
  change what players asked to change: stronger allies, bots that reach their
  command posts, and the same aim for both teams ([Classic AI](docs/ai.md),
  [Enhanced AI](docs/ai-enhanced.md)).
- **Sound.** Classic plays what the original game plays, mixed and placed by
  its rules; Enhanced keeps every one of those decisions and renders them as
  a spatial sound stage with rooms, occlusion and upscaled samples, at the
  same loudness ([audio](docs/audio.md)).
- **Picture.** In Battlefront (2004), terrain shading, foliage, water and
  steam effects, and the ocean and sky can each be drawn the 2004 way or the
  enhanced way ([rendering](docs/rendering.md)). From the next preview,
  Battlefront II (2005) battles draw that game's own Light Bloom, with a
  setting to turn it off.
- **Menus.** The front end can look like Battlefront (2004), Battlefront II
  (2005) or Eternal's own theme.

## Getting started

You need your own copy of *Star Wars: Battlefront* (2004) from Steam or GOG
(from the next preview, *Star Wars: Battlefront II* (2005) or both work
too). Download the app from
<https://battlefronteternal.com>:

- **Windows 10 and 11 (x64):** the installer. It installs for your user
  only and needs no administrator rights. Without a code-signing certificate
  Windows SmartScreen warns before it starts the app.
- **Linux (x64 and ARM64):** the AppImage (runs from any folder and adds
  itself to your app launcher) or the Debian/Ubuntu package.
- **macOS (Apple silicon, macOS 12.3 or later):** the disk image. Open it and
  drag the app to Applications. It is signed with a Developer ID certificate
  and notarized by Apple, so macOS opens it without a warning, and it updates
  itself like the other builds. Intel Macs are not supported yet. The game
  also builds from source on Apple silicon Macs
  ([docs/development.md](docs/development.md), "Building and running on
  macOS").

On first launch the app looks for the game in Steam, GOG Galaxy and Heroic,
says what it found, asks before preparing anything, and then prepares the
maps, armies, interface and sound into your user folder. This happens once
per game. Battlefront (2004) takes about 9 minutes on a 2011 quad-core PC
(playable after about 5), longer with the optional Enhanced sound. For each
game the preparing screen shows the time left, estimated for your computer. If it cannot find a game, choose the folder that holds
`GameData`, or import data prepared on another computer.

The app updates itself when it starts, without a reinstall or a password,
and keeps your settings, identity and prepared data. Updates never interrupt
a match.

### What you need

- a 64-bit Windows 10 or 11; a 64-bit Linux (x64 or ARM64) desktop with
  glibc 2.35 or newer (Ubuntu 22.04, Debian 12 or later); or an Apple silicon
  Mac with macOS 12.3 or later;
- a graphics card or chip with OpenGL ES 3.0 or OpenGL 3.3 and a current
  driver;
- about 4 GB of memory (a Battlefront (2004) battle uses about 1 GB);
- disk space for the prepared game data: about 7 GB for Battlefront (2004)
  and about 5 GB more for Battlefront II (2005) from the next preview.

Measured on an old machine with Battlefront (2004), with the preset the app
picked by itself:

| Machine | Settings | Result |
| --- | --- | --- |
| Intel HD Graphics 2000 (2011 desktop integrated graphics) | full screen 1920x1080, Minimum | Theed about 65 fps, Hoth about 62, Kashyyyk Docks about 52, Mos Eisley about 49, Geonosis about 48, Endor about 40 |

Busy maps (Endor, Mos Eisley, Geonosis) are the hardest. Minimum draws the
scenery out to the 2004 game's SHORT view distance; past that, a lower
RENDER SCALE or a lower full-screen RESOLUTION helps most on a weak chip.

## Single player and multiplayer

**Instant Action** builds a playlist of maps, eras and modes (from either
game or both in the next preview) and plays it against bots. A single player battle runs on its
own 60 Hz world thread inside the client, through the same pipeline as an
online battle.

**Multiplayer** lists online servers with their ping, finds LAN servers, and joins by address. The online server
list (`online.battlefronteternal.com`) introduces players to servers and
helps them connect: the client tries a direct path, punching through home
routers where it can, and goes through the relay
(`relay.battlefronteternal.com`) when no direct path answers. Battles run on
the servers themselves; the transport stays encrypted end to end through the
relay. From the next preview, a player without Battlefront II stays
connected through a server's 2005 battles, sitting them out until the next
battle. [MULTIPLAYER.md](MULTIPLAYER.md)
describes the design.

## Hosting a server

- **From the app:** MULTIPLAYER > CREATE starts the bundled `bfeserver` on
  your PC: for your network only (players find it under LAN), or public,
  listed online and joinable from anywhere, directly or through the relay,
  without opening ports on your router. The screen shows how players reach
  the server (IPv4, IPv6 or the relay). From the next preview the playlist
  can hold battles from either game or both, each marked with its game.
- **Dedicated server:** `bfeserver` runs headless on a Linux machine (x64
  or ARM64, no GPU). Run `bfeserver` with no arguments for its setup menu,
  `bfeserver --serve` to serve, and `bfeserver --help` for the options. It
  runs as a systemd service and prepares its own Battlefront (2004) data from
  a copy of the game you put on the server. A server set up with the updater
  follows the release channel by itself; otherwise the server archive's
  `install-server.sh` installs or updates it with one command
  ([server updates](docs/server-updates.md),
  [dedicated server](docs/native-server.md)). The game port (UDP and TCP
  18451 by default) must be open to players.
- **Battlefront II on a dedicated server (next preview):** the server plays 2005 battles
  from Battlefront II data the app prepared, in the `swbf2/.content` folder
  under its data folder ([dedicated server](docs/native-server.md)).
  Preparing the 2005 game on the server itself is not supported yet.

## Build from source

You need [rustup](https://rustup.rs) (it installs the pinned toolchain,
`1.99.0`, from `rust/rust-toolchain.toml` when `cargo` runs in `rust/`),
Python 3 for the preparation tools, and your own copy of the game. On Debian
and Ubuntu the build also needs:

```bash
sudo apt install build-essential pkg-config libfontconfig1-dev libasound2-dev
```

```bash
git clone https://github.com/centrevilletech/battlefront-eternal.git
cd battlefront-eternal/rust
cargo test --locked                        # unit tests; no game files needed
cargo run --release --locked -p bfe-client # builds and starts the game
cargo build --release --locked -p bfe-server  # the dedicated server, bfeserver
```

On first launch a source build finds the games as the packaged app does and
prepares them with the checkout's `tools/` and your system Python 3 (a step
whose Python packages are missing is skipped and reported). The
[development guide](docs/development.md) covers preparing data with the
Python tools, the command-line options, the generated code (`cargo xtask`)
and building the packages.

### Layout

- `rust/`: the Cargo workspace. `bfe-client` is the game (window, input,
  menus, battle and audio host); `bfe-server` is `bfeserver`; `sim` is the
  60 Hz world (units, vehicles, combat, AI, prediction, snapshots);
  `bfe-family` says which game a map, battle or entry belongs to;
  `bfe-script` runs the 2005 game's compiled mission scripts;
  `bfe-render`, `bfe-presentation`, `bfe-environment` and `bfe-pose` draw it;
  `audio`, `audio-control` and `audio-decode` play it; `bfe-shell`,
  `bfe-interface`, `bfe-battle-ui` and `bfe-ui` are the menus and battle
  screens; `bfe-transport` and `bfe-online` are the network; `bfe-desktop`
  finds and prepares the games and updates the app; `bfe-video` plays the
  movies; `xtask` generates and checks the generated code.
- `tools/`: the Python tools that read the original games' files and
  prepare them ([formats](docs/formats.md)).
- `build/`: packaging (Windows installer, AppImage, Debian package, macOS
  disk image, server archive, update payload) and CI scripts.
  `build/native/host-runtime/` holds the dedicated server's updater (a small
  Node.js program) and the launcher it starts `bfeserver` through; the Linux
  releases carry them so servers set up with the updater keep updating
  ([server updates](docs/server-updates.md)).
- `docs/`: design and behaviour notes, starting with the
  [architecture overview](docs/architecture.md).

## Documentation

- [Development and playtesting](docs/development.md)
- [Desktop app, packages and first launch](docs/desktop.md) and [updates](docs/desktop-updates.md)
- [Dedicated server](docs/native-server.md) and [server updates](docs/server-updates.md)
- [Architecture overview](docs/architecture.md) and [runtime architecture](docs/runtime-architecture.md)
- [Multiplayer design](MULTIPLAYER.md), the [UDP transport](docs/native-udp-transport.md) and the [network contract for both games](docs/network-two-games.md)
- [Battlefront II (2005): design and work plan](docs/battlefront-ii.md), [mission scripts](docs/scripts.md), [hero powers](docs/powers.md) and [melee](docs/melee.md)
- [Rendering](docs/rendering.md), [render performance](docs/render-performance.md) and [audio](docs/audio.md)
- [Classic AI](docs/ai.md) and [Enhanced AI](docs/ai-enhanced.md)
- [Vehicles](docs/vehicles.md), [menus](docs/menus.md) and the [HUD](docs/hud.md)
- [Game file formats](docs/formats.md) and the [feature audit](docs/feature-audit.md)

## Planned

Planned work, not built yet:

- **Rendering:** a modern rendering path for better visual effects, offered
  as an option beside OpenGL, once game parity work is done
  ([above](#planned-a-modern-rendering-path-as-an-option)).
- **Battlefront II:** the missions and differences listed under its maps,
  Galactic Conquest, the campaign and split screen.
- **Battlefront (2004):** Galactic Conquest and the Historical Campaign.
- **Everything else:** vehicle cockpits in first person, an Intel Mac build,
  signed Windows installers, easier dedicated server setup (including
  preparing Battlefront II on the server), leaderboards, and custom maps and
  mods.

Dates depend on testing and are not promised. The
[feature audit](docs/feature-audit.md) lists what the original game's data
holds that the engine does not do yet.

## Related repositories

- [bink-decoder](https://github.com/centrevilletech/bink-decoder): the
  movie decoder, a Rust crate of its own.
- [battlefront-eternal-server](https://github.com/centrevilletech/battlefront-eternal-server):
  getting a dedicated server: an install script (`get-bfeserver.sh`) and a
  Dockerfile.
- [battlefront-eternal-master](https://github.com/centrevilletech/battlefront-eternal-master):
  the server list and connection signaling.
- [battlefront-eternal-relay](https://github.com/centrevilletech/battlefront-eternal-relay):
  the relay service.

## Contributing

Bug reports and pull requests are welcome. Say which map you were playing
and what the original game does differently. Read [CONTRIBUTING.md](CONTRIBUTING.md)
first: never commit original game files or code taken from the original
games. Report security problems privately as described in
[SECURITY.md](SECURITY.md).

## License

Battlefront Eternal is free software, licensed under the GNU General Public
License, version 3 only (GPL-3.0-only). See [LICENSE](LICENSE). Third-party
components and their licenses are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Acknowledgments

Thanks to PrismaticFlower, whose
[swbf-unmunge](https://github.com/PrismaticFlower/swbf-unmunge) was a valuable
reference while documenting the game's data formats.

## Independent project and trademarks

Battlefront Eternal is an independent fan project. It is not affiliated with
or endorsed by Lucasfilm, Disney, Electronic Arts, Aspyr, Valve, GOG or the
developers and publishers of the original games. Star Wars, Battlefront,
Steam and GOG are trademarks of their respective owners.

No original game files are included in this repository or distributed with
the app. The app prepares its game data from the player's own Steam or GOG
copy of *Star Wars: Battlefront* (2004) or *Star Wars: Battlefront II*
(2005), and that prepared data stays on the player's machine.
