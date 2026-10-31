# Battlefront Eternal

Battlefront Eternal is a fan-made engine for *Star Wars: Battlefront* (2004),
with *Star Wars: Battlefront II* (2005) planned. It plays the original game's
maps, armies, sounds and music from your own Steam or GOG copy, through a new
renderer, a new audio engine, a rebuilt front end and new netcode.

This repository holds the desktop app and the engine: the WebGL renderer
(built on Three.js), the 60 Hz simulation and the bots, the audio engine,
the menus, the multiplayer client and the tools that prepare game data from
your copy. No original game files are included or distributed.

Much of Battlefront Eternal's code and documentation was written with AI
coding assistants, directed and play-tested by Josh Lambert.

Website and downloads: <https://battlefronteternal.com>

## What works today

Development previews run on Windows 10 and 11 (64-bit) and on Linux (x64 and
ARM64), and update themselves.

- **Battles.** Instant Action on all 16 original maps plus Jabba's Palace, in
  every era each one offers: 17 maps and 30 battles. Bots fight, capture
  command posts, drive and fly by the original game's rules.
- **Vehicles.** Speeders, tanks, starfighters, AT-ATs, AT-TEs and the other
  command vehicles, with their seats, weapons and damage effects.
  Destructible objectives, the locals and map interactives such as Ewok log
  traps and the snowspeeder's tow cable.
- **Graphics.** Classic and Enhanced looks: cascaded shadows, reflective
  water, weather, Enhanced destruction effects, upscaling, and presets from
  Minimum (integrated graphics of about 2011) up to High. Gameplay always
  runs at a fixed 60 Hz, whatever the frame rate.
- **Sound.** Original (2004) and Enhanced (2026) modes, with 3D audio on
  headphones and speaker layouts up to 7.1.4.
- **Multiplayer.** Server-authoritative battles at 60 Hz over an encrypted
  UDP transport, up to 32 versus 32 with bots filling empty slots. An online
  server browser with measured ping, LAN games, direct connect and hosting
  from the app. No account needed.
- **Your copy stays yours.** The app finds the game in Steam, GOG Galaxy or
  Heroic, prepares about 7 GB of data in your user folder once, and only
  ever reads your installation.

## Planned

*Star Wars: Battlefront II* (2005) maps, units, vehicles, modes and
interface; mixed playlists from both games; Galactic Conquest and the
Historical Campaign; macOS builds and signed Windows installers; easier
dedicated server setup; leaderboards; custom maps and mods. Dates depend on
testing and are not promised.

## Play

Download the app from <https://battlefronteternal.com>. You need your own
copy of *Star Wars: Battlefront* (2004) from Steam or GOG.

## Build from source

You need Node.js 24 with npm, Python 3, and your own copy of the game.

```bash
git clone https://github.com/centrevilletech/battlefront-eternal.git
cd battlefront-eternal
npm ci
npm test          # unit tests; no game files needed
npm run desktop   # builds and starts the desktop app
```

`npm run desktop` bundles the dedicated server runtime from
[battlefront-eternal-server](https://github.com/centrevilletech/battlefront-eternal-server)
at the revision pinned in `build/host-runtime.json`, builds the engine and
starts the app. On first launch it finds your copy of the game and prepares
its data.

The [development guide](docs/development.md) covers the browser development
build, preparing data with the Python tools, controls and testing.

## Documentation

- [Development and playtesting](docs/development.md)
- [Desktop app and packaging](docs/desktop.md)
- [Dedicated server updates and the server archive](docs/server-updates.md)
- [Game file formats](docs/formats.md)
- [Runtime architecture](docs/runtime-architecture.md)
- [Multiplayer design](MULTIPLAYER.md) and the [UDP transport](docs/native-udp-transport.md)
- [Rendering](docs/rendering.md) and [audio](docs/audio.md)
- [Classic AI](docs/ai.md) and [Enhanced AI](docs/ai-enhanced.md)
- [Vehicles](docs/vehicles.md) and [menus](docs/menus.md)

## Related repositories

- [battlefront-eternal-server](https://github.com/centrevilletech/battlefront-eternal-server):
  the dedicated server, `bfeserver`.
- [battlefront-eternal-master](https://github.com/centrevilletech/battlefront-eternal-master):
  the server directory and connection signaling.
- [battlefront-eternal-relay](https://github.com/centrevilletech/battlefront-eternal-relay):
  the relay for players who can't connect directly.

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
