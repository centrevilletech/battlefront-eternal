# Battlefront Eternal

Battlefront Eternal is a fan-made engine for *Star Wars: Battlefront* (2004),
written in Rust. It plays the original game's maps, armies, sounds, music and
movies from your own Steam or GOG copy, through a new renderer, a new audio
engine, a rebuilt front end and new netcode. *Star Wars: Battlefront II*
(2005) is planned.

You bring the game: no original game files are included in this repository
or distributed with the app. On first launch the app finds your copy and
prepares its data on your computer.

Much of Battlefront Eternal's code and documentation was written with AI
coding assistants, directed and play-tested by Josh Lambert.

Website and downloads: <https://battlefronteternal.com>

## What works today

Development previews run on Windows 10 and 11 (64-bit) and on Linux (x64 and
ARM64), and update themselves.

- **Battles.** Instant Action on all 16 original maps plus Jabba's Palace, in
  every era each one offers: 17 maps and 30 battles. Bots fight, capture
  command posts, drive and fly by the original game's rules. The locals
  (Tusken Raiders, Jawas, Gungans, Wookiees, Ewoks and the rest) join in where
  the missions place them.
- **Vehicles.** Speeders, tanks, starfighters, AT-ATs, AT-TEs and the other
  command vehicles, with their seats, weapons, mobile spawn points and damage
  effects. Destructible objectives, turrets and map interactives such as the
  Ewok log traps and the snowspeeder's tow cable.
- **Classic and Enhanced.** Each battle can be played the 2004 way or with this
  project's additions (see below).
- **Graphics.** Cascaded shadows, reflective water, weather, destruction
  effects, upscaling and four quality presets (Minimum, Performance, Balanced
  and High Quality). On first launch the app measures the graphics card and picks a
  starting preset. Gameplay always runs at a fixed 60 Hz, whatever the frame
  rate.
- **Sound.** Original (2004) and Enhanced (2026) modes, with 3D audio on
  headphones and speaker layouts up to 7.1.4, and a light stereo mix of the
  original for slow PCs.
- **Movies.** The game's own menu movies and map flyovers play straight from
  your game folder, decoded by this project's
  [Bink decoder](https://github.com/centrevilletech/bink-decoder); nothing is
  converted when the game is prepared.
- **Multiplayer.** Server-authoritative battles at 60 Hz over an encrypted
  UDP transport, up to 32 versus 32 with bots filling empty slots. An online
  server browser with measured ping, LAN games, direct connect, and hosting
  from the app. No account needed.
- **Your copy stays yours.** The app finds the game in Steam, GOG Galaxy or
  Heroic (or a folder you choose), prepares its data in your user folder, and
  only ever reads your installation. Prepared data can be exported and
  imported on another computer.
- **Friends.** Steam can show you as playing Battlefront Eternal, and Discord
  Rich Presence shows the map and server.

## Planned

*Star Wars: Battlefront II* (2005) maps, units, vehicles, modes and
interface; mixed playlists from both games; Galactic Conquest and the
Historical Campaign; vehicle cockpits in first person; macOS builds; signed
Windows installers; easier dedicated server setup; leaderboards; custom maps and mods; a Vulkan
renderer backend, followed by FSR upscaling on every GPU and DLSS as an
optional add-on for RTX cards. Dates depend on testing and are not promised.
The [feature audit](docs/feature-audit.md) lists what the original game's
data holds that the engine does not do yet.

## Play

You need your own copy of *Star Wars: Battlefront* (2004) from Steam or GOG.
Download the app from <https://battlefronteternal.com>:

- **Windows 10 and 11 (x64):** the installer. It installs for your user
  only and needs no administrator rights. Without a code-signing certificate
  Windows SmartScreen warns before it first starts.
- **Linux (x64 and ARM64):** the AppImage (runs from any folder and adds
  itself to your app launcher) or the Debian/Ubuntu package.
- **macOS:** not yet; it is next on the list.

On first launch the app looks for the game in Steam, GOG Galaxy and Heroic,
asks before preparing anything, and then prepares the maps, armies, interface
and sound into your user folder. This takes a while once; allow about 7 GB of
free space. If it cannot find the game, choose the folder that holds
`GameData`, or import data prepared on another computer.

The app updates itself when it starts, without a reinstall or a password,
and keeps your settings, identity and prepared data. Updates never interrupt
a match.

### What you need

The measured floor is a 2011 integrated graphics chip, so most PCs from the
last decade play it:

- a 64-bit Windows 10 or 11, or a 64-bit Linux (x64 or ARM64) desktop;
- a graphics card or chip with OpenGL ES 3.0 or OpenGL 3.3 and a current
  driver;
- about 4 GB of memory (a battle uses about 1 GB);
- about 7 GB of disk space for the prepared game data.

Measured on an old machine, with the preset the app picked by itself:

| Machine | Settings | Result |
| --- | --- | --- |
| Intel HD Graphics 2000 (2011 desktop integrated graphics) | full screen 1920x1080, Minimum | Theed about 65 fps, Hoth about 61, Kashyyyk Docks about 45, Mos Eisley about 37, Endor about 19 (about 31 with the SHORT view distance) |

Busy maps (Endor, Mos Eisley, Geonosis) are the hardest. On a weak chip a
lower RENDER SCALE or the SHORT VIEW DISTANCE helps most.

## Classic and Enhanced

Battlefront Eternal keeps the 2004 game as the reference and offers its
additions as options, each one CLASSIC or ENHANCED in the menus:

- **Bots.** Classic bots are the 2004 game's AI, quirks included. Enhanced
  bots keep its navigation, weapons, vehicles and squad commands and change
  what players asked to change: stronger allies, bots that reach their command
  posts, and the same aim for both teams ([Classic AI](docs/ai.md),
  [Enhanced AI](docs/ai-enhanced.md)).
- **Sound.** Classic plays what the 2004 game plays, mixed and placed by its
  rules; Enhanced keeps every one of those decisions and renders them as a
  spatial sound stage with rooms, occlusion and upscaled samples, at the same
  loudness ([audio](docs/audio.md)).
- **Picture.** Terrain shading, foliage, water and steam effects, and the
  ocean and sky can each be drawn the 2004 way or the enhanced way
  ([rendering](docs/rendering.md)).
- **Menus.** The front end can look like Battlefront (2004), Battlefront II
  (2005) or Eternal's own theme.

## Single player and multiplayer

**Instant Action** builds a playlist of maps and eras and plays it against
bots. A single player battle runs on its own 60 Hz world thread inside the
client, through the same pipeline as an online battle.

**Multiplayer** lists online servers with their ping, finds LAN servers, and
joins by address. The online server list (`online.battlefronteternal.com`)
only introduces players to servers; battles run on the servers themselves. [MULTIPLAYER.md](MULTIPLAYER.md) describes the design.

## Hosting a server

- **From the app:** MULTIPLAYER > HOST starts the bundled `bfeserver` on
  your PC; players on your network find it under LAN.
- **Dedicated server:** `bfeserver` runs headless on a Linux machine (x64
  or ARM64, no GPU). Run `bfeserver` with no arguments for its setup menu,
  `bfeserver --serve` to serve, and `bfeserver --help` for the options. The
  server archive from each release installs as a systemd service that updates
  itself and prepares its own data from a copy of the game you put on the
  server ([server updates](docs/server-updates.md),
  [dedicated server](docs/native-server.md)). The game port (UDP and TCP
  18451 by default) must be open to players.

## Build from source

You need [rustup](https://rustup.rs) (it installs the pinned toolchain,
`1.99.0`, from `rust/rust-toolchain.toml` on the first `cargo` command),
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

On first launch a source build finds the game as the packaged app does and
prepares it with the checkout's `tools/` and your system Python 3 (a step
whose Python packages are missing is skipped and reported). The
[development guide](docs/development.md) covers preparing data
with the Python tools, the command-line options, the generated code
(`cargo xtask`) and building the packages.

### Layout

- `rust/`: the Cargo workspace. `bfe-client` is the game (window, input,
  menus, battle and audio host); `bfe-server` is `bfeserver`; `sim` is the
  60 Hz world (units, vehicles, combat, AI, prediction, snapshots);
  `bfe-render`, `bfe-presentation`, `bfe-environment` and `bfe-pose` draw it;
  `audio`, `audio-control` and `audio-decode` play it; `bfe-shell`,
  `bfe-interface`, `bfe-battle-ui` and `bfe-ui` are the menus and battle
  screens; `bfe-transport` and `bfe-online` are the network; `bfe-desktop`
  finds and prepares the game and updates the app; `bfe-video` plays the
  movies; `xtask` generates and checks the generated code.
- `tools/`: the Python tools that read the original game's files and prepare
  them ([formats](docs/formats.md)).
- `build/`: packaging (Windows installer, AppImage, Debian package, server
  archive, update payload) and CI scripts.
- `docs/`: design and behaviour notes, starting with the
  [architecture overview](docs/architecture.md).

## Documentation

- [Development and playtesting](docs/development.md)
- [Desktop app, packages and first launch](docs/desktop.md) and [updates](docs/desktop-updates.md)
- [Dedicated server](docs/native-server.md) and [server updates](docs/server-updates.md)
- [Architecture overview](docs/architecture.md) and [runtime architecture](docs/runtime-architecture.md)
- [Multiplayer design](MULTIPLAYER.md) and the [UDP transport](docs/native-udp-transport.md)
- [Rendering](docs/rendering.md), [render performance](docs/render-performance.md) and [audio](docs/audio.md)
- [Classic AI](docs/ai.md) and [Enhanced AI](docs/ai-enhanced.md)
- [Vehicles](docs/vehicles.md), [menus](docs/menus.md) and the [HUD](docs/hud.md)
- [Game file formats](docs/formats.md) and the [feature audit](docs/feature-audit.md)

## Related repositories

- [bink-decoder](https://github.com/centrevilletech/bink-decoder): the
  movie decoder, a Rust crate of its own.
- [battlefront-eternal-server](https://github.com/centrevilletech/battlefront-eternal-server):
  the dedicated server's service units and install notes.
- [battlefront-eternal-master](https://github.com/centrevilletech/battlefront-eternal-master):
  the server list and connection signaling.
- [battlefront-eternal-relay](https://github.com/centrevilletech/battlefront-eternal-relay):
  the relay service.

## Contributing

Bug reports and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md)
first: never commit original game files or code taken from the original
games. Report security problems privately as described in
[SECURITY.md](SECURITY.md).

## License

Battlefront Eternal is free software, licensed under the GNU General Public
License, version 3 only. See [LICENSE](LICENSE). Third-party components and
their licenses are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

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
copy of *Star Wars: Battlefront* (2004), and that prepared data stays on the
player's machine.
