# Battlefront II (2005) support: plan

This is a plan. Nothing in it is implemented unless a line says so. It
extends the "Two games from day one" section of
[MULTIPLAYER.md](../MULTIPLAYER.md) into concrete work, and it names the code
that has to change. Dates depend on testing and are not promised.

The Steam editions of *Star Wars: Battlefront* (2004) and *Star Wars:
Battlefront II* (2005) are the reference for behaviour. As with the 2004
game, Battlefront Eternal ships no game data: everything is read from the
player's own copy.

## Goals

1. **One client, either game or both.** A player can prepare a copy of the
   2004 game, a copy of the 2005 game, or both. The client plays the maps it
   has prepared and can join servers on those maps.
2. **One server, either game or both.** `bfeserver` runs with 2004 maps only,
   2005 maps only, or both, depending on the copies its operator supplied.
3. **The battle follows the map's game.** Controls, loading screen, HUD,
   spawn screen, scoreboard, soldier movement (sprint and the forward roll in
   the 2005 game, prone and the dive in the 2004 game) and the walk cycle are
   those of the game the map comes from.
4. **Mixed playlists.** A playlist, offline or on a server, can alternate
   maps from both games with no extra step for the player.
5. **The 2004 game keeps working exactly as it does today.** No change made
   for Battlefront II may alter how a Battlefront map loads, plays or
   replicates.

### Opening targets

Two maps prove the hard parts early, one on the ground and one in space:

| Target | Map and mode | Mission | What it proves |
| --- | --- | --- | --- |
| A | Mos Eisley, Hero Assault | `swbf2/tat2:gcw:eli` | A 2005 ground map, 2005 soldier movement, playable heroes with lightsaber combat, a mode with no command posts, the 2005 interface |
| B | Space Mygeeto, Space Assault | `swbf2/spa6:cw:ass` | A space map, capital ships with destructible systems and shields, hangars and boarding, starfighter classes, a points mode, positions far outside ground-map ranges |

### Not in the opening scope

Galactic Conquest, the campaign ("Rise of the Empire"), the remaining modes
(Conquest, capture the flag, Hunt, XL, the ground Assault), awards and bonus
weapons, split screen, custom maps and mods. The design below leaves room for
each; the Conquest, CTF and Hunt modes follow soon after the opening targets
because they reuse the same script runtime.

## Where the code stands today

Implemented:

- A `GameFamily` type (`swbf1`, `swbf2`) in the shell's launch and playlist
  code (`rust/bfe-shell/src/launch.rs`) and in the battle UI
  (`rust/bfe-battle-ui/src/shapes.rs`). The two are separate copies of one
  idea, and `swbf2` reports itself unavailable.
- Map ids with a family prefix parse in the shell (`swbf2/hot1`), and playlist
  entries carry family, map and mode. Instant Action has a game picker with
  Battlefront II disabled.
- `bfeserver` accepts `swbf2/` playlist entries in its menu and then refuses
  them when it builds the rotation ("BF2 simulation is not implemented").
- The flight model already has a space setting (no ceiling, no wing
  levelling, vertical bounds), tested only against synthetic collision.
- Melee strikes exist for the 2004 game's AI heroes. There is no blocking,
  deflection, force power or playable hero.

Not implemented, and assumed to be the 2004 game everywhere below the shell:

- Finding and validating a copy: one Steam app, one `game_dir`, and a
  validator that rejects a Battlefront II folder on purpose.
- Preparation: the tools write `maps/`, `sides/`, `common/` and `audio/` with
  no family in the path, read Lua 4.0 only, and carry the 2004 game's era and
  side tables.
- Content loading and the content fingerprint: bare `/maps/<map>/...` paths.
- Rules: Conquest is written in Rust (`rust/sim/src/battle/rules.rs`) and is
  the only mode. A map with no command posts would never end. There is no
  script runtime anywhere.
- Movement: three postures, a sideways dive, a jetpack. No sprint, stamina or
  roll, and no per-battle rules profile to hang them on.
- Network: the join message and the map change carry no family or mode; the
  snapshot quantizes positions to one fixed range (2 km square, 700 m tall);
  a content mismatch refuses the join.
- Presentation: one set of key bindings, a HUD whose layout is Rust constants
  for the 2004 game, one global soldier animation set, and a loading screen
  whose look follows the player's menu theme.

## What the 2005 game's files look like

Read from a Steam copy with the project's own archive reader. This section
describes layout and formats only.

| Area | 2004 game | 2005 game | Consequence |
| --- | --- | --- | --- |
| Install | `GameData/Data/_LVL_PC`, `Battlefront.exe` | `GameData/data/_lvl_pc`, `BattlefrontII.exe`; no `AddOn` folder until the player adds one | A second validator; paths already match without regard to case |
| Shared archives | `Common/<LANG>/common.lvl`, `core.lvl`, `mission.lvl` | `common.lvl`, `core.lvl`, `mission.lvl`, plus `ingame.lvl` (soldier animations, effects, HUD) and `inshell.lvl` | A per-family list of archives to read |
| Maps | One archive per map; one world | One archive per map holding the base world plus one layer per mode (Mos Eisley has 13 world chunks) | The importer composes base world plus the mission's layer |
| New chunk types | | Lights (`lght`), portals (`port`), cloth (`CLTH`), melee combos (`comb`), HUD definitions (`hud_`), lower-case `path`, flyer planning (`plnp`) | New readers; several are needed for the opening targets |
| Sides | A few sub-archives per side | One named sub-archive per class (the Republic side has about 50), loaded by name from the mission script; heroes in their own sides | Extraction is driven by what each mission loads |
| Animations | Global banks in `common.lvl` | Banks in `ingame.lvl` and inside each side, per unit (heroes carry their own sets) | Animation tables per family and per unit |
| Mission scripts | Lua 4.0 bytecode, one setup function | Lua 5.0 bytecode, 138 scripts: per-mission scripts plus shared mode scripts (Conquest, CTF, one-flag CTF, Assault, Space Assault, team deathmatch) and helpers (linked shields, turrets, destroyables) | See "Scripts" below |
| Script numbers | Double | 32-bit float, 32-bit sizes | A stock Lua 5.0 build cannot load these chunks |
| Text | `Locl` tables in `core.lvl` | Same format | The existing reader works |
| Sound | Per-planet archives plus `common.bnk` | Same shape, plus a `global.lvl` | Readers to be confirmed sample by sample |
| Movies | `.mvs` archives of Bink 1 movies | Same container; every movie checked is Bink 1 revision `i` at 800 x 600. Shell movies carry 48 kHz stereo audio; the in-game and trailer movies carry 44.1 kHz | The video revision is one the decoder supports. The 44.1 kHz audio path has not been checked against a reference and needs a test before those movies are used |
| Loading art | `Load/load.lvl` | `load/<map>.lvl` per map plus `load/common.lvl` | A family loading screen with its own art source |

The two opening missions, as the scripts name them: `tat2g_eli` builds a team
deathmatch between a hero team and a villain team with a score limit, and
`spa6c_ass` builds a Space Assault between the Republic and the CIS in which
each capital ship has engines, life support, a bridge, communications,
sensors, a shield generator, internal systems and escort frigates.

## Decisions

### 1. Ids, playlists and modes

- Every map, mission, side and class id names its family: `swbf1/hot1`,
  `swbf2/hot1`. A bare id keeps meaning `swbf1`.
- A playlist entry is `family/map:era:mode`, for example `swbf2/tat2:gcw:eli`
  and `swbf2/spa6:cw:ass`. In the 2005 game this maps directly onto the
  mission script name (`tat2g_eli`).
- `bfeserver` today prompts for `family/map:side:mode` (`swbf1/nab2:rep:conquest`).
  Era becomes the canonical second field; a side in that position keeps
  working and names its era, so saved playlists stay valid.
- The two copies of `GameFamily` become one type in a crate both the shell
  and the battle UI already depend on.
- Family and mode travel the whole way: `BattleRequest` to `BattleOptions`, to
  the world's admission options, to the server's rotation query, to the map
  change message and the server's info reply.

### 2. Content layout

- The 2004 game's prepared data stays exactly where every released build
  keeps it: `<content>/{maps,sides,common,audio}`. Nothing is moved, so a
  released build and a newer one can share one folder in either direction,
  and a server's read-only or shared data folder needs no change.
- The 2005 game's prepared data lives in its own subfolder:
  `<content>/swbf2/.content/{maps,sides,common,audio}`. The hidden inner
  folder and a small family marker file keep a released build from ever
  adopting it as 2004 data: released builds skip hidden folders when they
  search for content, and the 2005 preparation record has its own file
  name.
- One function maps a family to its root (`swbf1` to `<content>`, `swbf2`
  to `<content>/swbf2/.content`). The loaders take that root, so every path
  inside them stays as it is.
- The 2004 game's content fingerprint does not change (see "Protecting the
  2004 game"). The family is added to the fingerprint only for `swbf2`.
- Settings follow the same rule: the keys a released build writes remain the
  2004 game's storage, and only the 2005 game's values are stored under a
  new key. The preparation record and content version stamp are per family
  because each lives in its family's root. The tools get the family from
  the client (`BFE_FAMILY`), and a per-family profile supplies what is typed
  in today: archive list, era and side tables, program name, language folder
  rule and Lua loader.

An earlier draft of this plan moved existing 2004 data once into a `swbf1/`
folder for symmetry. Review of that approach found too many ways for an
interrupted move, a downgrade or a running server to strand a player's
prepared data, so the asymmetric layout above replaces it.

### 3. Finding the player's copies

- Discovery looks for both Steam apps, the GOG and Heroic entries of both
  games, and a folder the player picks. Each candidate is validated by the
  layout of its own game, so a Battlefront II folder is never handed to the
  2004 validator and the reverse.
- The setup and settings screens list each game as prepared, found but
  not prepared, or not found. Either game alone is a complete setup.
- A server operator supplies copies the same way as today, one per game.

### 4. Scripts: run the player's mission scripts

MULTIPLAYER.md left this open: run the 2005 game's Lua, or port each mode.

**Decision: run the scripts from the player's copy in a small Lua 5.0
interpreter written in Rust, inside the simulation, on the authority only.**

Why:

- The modes are scripts, and so is every map's glue. Space Mygeeto wires
  shields to a generator, turrets to a mainframe and frigates to kill
  callbacks in its own script. Porting modes would mean porting 138 scripts
  and would rule out custom maps later, since a mod brings its own scripts.
- The rules then are the original rules, not a reading of them.
- Lua 5.0 is small (35 instructions). The reference implementation is MIT
  licensed, so a port with attribution is allowed.

How:

- A new crate, `rust/bfe-script`: loader for the 2005 game's compiled chunks
  (float numbers, 32-bit sizes), the virtual machine, the base, string and
  table libraries the scripts use, and a host interface. No C code, so every
  shipping target builds it the same way.
- Arithmetic is 32-bit float, which is exact across architectures; the math
  library goes through `v8math` like the rest of the simulation. Table
  iteration order is defined by the interpreter, not by a hash of addresses.
  This keeps the simulation's cross-architecture byte equality.
- The simulation implements the script functions the game exposes (several
  hundred exist; each mode uses a few dozen). Events such as an object
  killed, a timer elapsed or a region entered are queued during the tick and
  delivered at one fixed point in the tick order.
- Scripts run only where the authority runs: the server, or the local world
  thread in Instant Action. A client never runs mission scripts. What a
  script changes (scores, object properties, markers, messages, voice-over
  cues) reaches clients as replicated state and events.
- Preparation uses the same interpreter with a recording host: it runs a
  mission's setup and records which sides, classes, sounds and world layers
  it asks for. That replaces symbolic reading for the 2005 game. The 2004
  game keeps its current Lua 4.0 reader untouched.
- Rules sit behind one trait in the simulation. The 2004 Conquest code is one
  implementation, unchanged. The scripted host is the other. The parts of a
  mode the 2005 game implements natively and only steers from script (Space
  Assault's scoring of systems, for example) are written in Rust behind the
  functions the scripts call.

### 5. A family supplies the differences, the engine stays shared

The clock, collision, commands, snapshots, transport, renderer and audio
engine are shared. A family supplies:

| Part | `swbf1` | `swbf2` |
| --- | --- | --- |
| Importer profile | Today's tools | New profile, new chunk readers |
| Rules | Conquest in Rust | Mission scripts on the script runtime |
| Movement profile | Stand, crouch, prone; sideways dive; jump; jetpack | Stand, crouch; sprint on an energy bar; forward and side rolls; jump; jetpack; hero jumps |
| Input actions | Today's list | Adds sprint and roll and the hero and space actions; no prone |
| Default bindings | Today's | The 2005 game's defaults, saved separately |
| HUD | Layout in Rust, 2004 art | Built from the game's own HUD definitions where they are complete, 2005 art |
| Spawn screen | The spawn launcher (the front end's look; Classic view on request) | The same spawn launcher, fed from the mission |
| Scoreboard, pause menu, end screens | 2004 Classic views | 2005 Classic views |
| Loading screen | The 2004 sequence, styled by the menu theme | One backdrop per load with tips and loading lights, styled by the menu theme; an Enhanced 3D backdrop is planned |
| Soldier animation | One global set | Per-unit sets, with sprint, roll and lightsaber sets |
| Audio | 2004 director and voice-over tags | 2005 music and voice-over mapping |

The numbers behind the 2005 movement (sprint speed, energy cost and
recovery, roll distance and timing) come from the class data in the
player's files where the game stores them there, and from measurements of
the Steam game otherwise.

### 6. The battle's look follows the map, the theme styles it

Today the loading screen and the page-style spawn screen and scoreboard
follow the player's `menuTheme` (`eternal`, `bf1`, `bf2`). The 2004 loading
screen is already a hybrid: the 2004 game's sequence, drawn with the lights,
fonts and text of the theme the player picked. The 2005 game gets the same
treatment:

- The front end keeps following `menuTheme`.
- In battle, the family of the map decides the structure: which loading
  screen, HUD, Classic spawn screen, scoreboard and pause menu. This switch
  happens behind the loading screen of the incoming map.
- **The 2005 loading screen** works as the 2005 game's does: one
  backdrop picked for each load from the map's loading pictures (the copy
  holds a set per map, four for Mos Eisley and thirteen shared by the space
  maps) and held for the whole load, a tips box whose tip changes on the
  game's own timer, and loading lights that fill over the screen's fixed
  length. It has none of the 2004 screen's planet tracking sequence or its
  beeps. The lights, fonts and text styling come from the selected theme, as
  they do on the 2004 screen.
- **Planned, Enhanced:** the game's loading pictures are low resolution and
  stretch on modern displays. An Enhanced loading backdrop is planned that
  draws the incoming map itself in 3D, empty, so the picture is sharp at any
  resolution. The Classic screen above stays the default behaviour of a
  Classic setup, and the fixed length applies to both.
- The Eternal page views stay available as the player's explicit choice
  through the existing view settings; they then apply to both families.
- The loading screen keeps a fixed length that is the same on every client,
  for both families.

### 7. Joining, and servers that run both games

- A server advertises its families and its rotation. The browser's GAME
  column shows BF, BFII or both, and marks the maps of a rotation the player
  has prepared.
- A player can join when they have the map being played. The join message
  names the family along with the protocol, simulation and content values it
  carries today.
- On a mixed rotation, the content check runs again at every map change.
  A player without the incoming map's game sits that map out in a lobby view
  (scoreboard and chat, no world) and plays again on the next map they have.
  This is the small form of the spectator design in MULTIPLAYER.md and
  replaces today's hard refusal for content alone. A protocol or simulation
  mismatch still refuses the join.
- `bfeserver` derives its mode from what its operator prepared: one game or
  both. The playlist editor only offers maps of prepared games, and a
  playlist entry for a game that is not prepared is an error at startup, not
  at map change.
- Each match already starts a fresh world on the server, so alternating
  families between matches needs no special case there.

### 8. Wire changes

Planned as one protocol revision, released together for client and server:

- Family and mode in the hello, the map change and the info reply.
- Positions quantized against each map's bounds (the parameter exists; the
  callers pass a fixed default). Space maps need this; ground maps keep
  their current precision.
- The input action list becomes the union of both games' actions; a family
  masks the ones it does not have.
- State only the 2005 game has (energy, block state, lock-on, system health
  and shields, mode score) is added as records a `swbf1` battle never sends.
- A check of the entity slot count against the busiest space map.

## Work by area

| Area | Code | Change |
| --- | --- | --- |
| Discovery | `rust/bfe-desktop/src/discover.rs`, `settings.rs`, `prepare.rs` | Per-family app ids, validators, game folders and preparation records |
| Tools | `tools/zero_paths.py`, `tools/prepare.py`, `tools/extract.py`, `tools/zero_shell.py`, `tools/zero_sound.py`, `tools/zero_language.py` | Family root and profile; 2005 readers for worlds with layers, lights, cloth, combos, HUD definitions, per-class side archives, animations, sound, loading art |
| Script runtime | new `rust/bfe-script` | Loader, interpreter, libraries, host interface, recording host for preparation |
| Content | `rust/content`, `rust/bfe-content-native` | Family root; mission model with mode and per-family eras; fingerprint rule; per-unit animation sets |
| Rules | `rust/sim/src/battle/` | A rules trait; Conquest behind it unchanged; scripted host; team deathmatch and Space Assault natives |
| Movement and combat | `rust/sim/src/movement/`, `units/`, `combat/` | Family movement profile; energy; roll; melee combos from data, block and deflection; force powers as weapons |
| Flight and space | `rust/sim/src/flight/`, `vehicles/` | Real space maps; hangar takeoff and landing; ship systems, shields and turrets as live objects; lock-on |
| AI | `rust/sim/src/ai/` | Goal-driven bots for modes without command posts; melee; space pilots. The 2004 AI is untouched |
| Wire and server | `rust/sim/src/wire/`, `rust/bfe-transport`, `rust/bfe-server` | The protocol revision above; rotation with family and mode; per-family content roots; sit-out state |
| Client battle | `rust/bfe-client/src/app/battle.rs`, `input.rs` | Choose bindings, HUD, views, animation tables and audio from the battle's family |
| HUD and views | `rust/bfe-render/src/hud_model/`, `rust/bfe-battle-ui` | A second HUD model and the 2005 Classic views behind the existing ports |
| Shell | `rust/bfe-shell`, `rust/bfe-interface`, `rust/bfe-client/src/app/slint_ia.rs` | Map lists grouped and badged by game; ids with family everywhere (bare `hot1` collides today); 2005 loading screen; controls page per game |
| Animation | `rust/bfe-pose`, `rust/bfe-presentation`, `rust/bfe-content-native/src/units.rs` | Table shapes per family: sprint gait, rolls, no prone, saber sets |
| Movies | `rust/bfe-video` | One movie library per family; verify 44.1 kHz audio |

## Milestones

Each milestone ends with a check that can be run again.

**M0. Family seams, no new behaviour.** Family and mode carried end to end;
a content root per family (the 2004 root unchanged); one `GameFamily`; the rules
trait with Conquest behind it; per-map wire bounds; per-family settings and
discovery. Check: every 2004 map loads and plays, and the world digests,
content fingerprints and cross-architecture byte equality are unchanged.

**M1. Preparing a 2005 copy.** The client finds and validates a Battlefront
II copy and prepares Mos Eisley and Space Mygeeto: world with the mission's
layer, lights, sky, collision, sides and classes the missions load, sounds,
text, loading art. Check: the import audit resolves every class, model and
texture the two missions name, or lists what the retail files do not hold.

**M2. Script runtime.** `bfe-script` loads and runs the two missions' setup
with the recording host, and the shared mode scripts load. Check: a test
corpus of small compiled chunks plus every script in `mission.lvl` loading
without error, identical results on x86-64 and ARM64.

**M3. Mos Eisley on foot.** The map renders and a 2005 soldier walks it in
Instant Action with the 2005 movement profile, animation set and bindings.
Check: the map loads and plays headless; side-by-side captures against the
Steam game for the walk, sprint and roll.

**M4. Target A: Hero Assault.** Playable heroes with lightsaber combos,
block and deflection, the force powers the opening rosters use; the team
deathmatch script running to a win; the 2005 HUD, spawn screen, scoreboard
and loading screen; bots that fight; the same match on a server.
Check: a full offline match and a full online match end with a winner.

**M5. Target B: Space Assault.** The space world; capital ships with
interiors; systems, shields, frigates and turrets; pilot and marine classes;
starfighter classes with lock-on; hangars and boarding; Space Assault
scoring; the space HUD; bot pilots. Check: a full offline match and a full
online match end with a winner; positions stay exact at the map's edges.

**M6. Both games together.** Mixed playlists offline and on a server; the
browser's game column and filters; the sit-out state; `bfeserver` with one
game or both. Check: a rotation alternating 2004 and 2005 maps runs for
several cycles with one client that has both games and one that has only
one.

After M6: Conquest, CTF and Hunt on the 2005 maps, then the rest of the map
list, then the items under "Not in the opening scope".

M1 and M2 are independent and can proceed in parallel after M0. M3 needs
M1. M4 and M5 need M2 and M3.

## Protecting the 2004 game

- **Byte-for-byte checks at every seam.** The M0 changes are refactors. The
  existing world digest, content fingerprint and cross-architecture tests
  must give the same values before and after each one. A change that moves a
  digest is not a seam; it is a behaviour change and is rejected.
- **`swbf1` code paths stay as they are.** The 2004 Conquest rules, Lua 4.0
  reader, AI, HUD and movement constants are wrapped, not rewritten. Where a
  shared function gains a family parameter, the `swbf1` branch is the
  existing code.
- **One fingerprint and one folder for 2004 content.** The 2004 game's
  content hash and its place on disk do not change, so released clients and
  servers keep agreeing and can be mixed with newer ones.
- **One protocol revision.** The wire changes land together, in a release
  where client and server update in step.
- **Battlefront II stays unavailable in released builds until it works.**
  The existing availability check on the family is the switch. Seams can be
  released as they land; 2005 features land behind the check and are
  switched on per milestone.
- **The same bar for both games.** "Every enabled map loads and plays a full
  match headless" is the gate for a release, for each family.
- **Fixes to the 2004 game are named, not hidden.** Checking every change
  against the released build has turned up defects in the 2004 game itself
  (objects left undrawn in busy battles, a preparation race in Enhanced
  Sound). Those fixes ship with this work, each as its own change with its
  own test. Everything else about the 2004 game stays as released.

## Existing open-source work

Other people have documented much of the 2005 game's data. What can be used,
and how:

| Project | License | Use |
| --- | --- | --- |
| [LibSWBF2](https://github.com/Ben1138/LibSWBF2) | GPL-3.0 | C++ readers for the 2005 game's archives: models, textures, terrain, worlds, sound banks, animations. Compatible; port readers to the Python tools and Rust with attribution |
| [swbf-unmunge](https://github.com/PrismaticFlower/swbf-unmunge) | MIT | Handles both games' chunk formats behind one switch, which makes it the best map of where the two differ. Port with attribution |
| [lua5.0-swbf2-x64](https://github.com/Ben1138/lua5.0-swbf2-x64) | GPL-3.0 | Lua 5.0 changed to load the 2005 game's compiled scripts. Reference for the chunk layout and for testing the Rust interpreter against |
| [Lua 5.0](https://www.lua.org/versions.html#5.0) | MIT | Reference implementation the interpreter is ported from |
| [SWBF2-Lua-API](https://github.com/Gametoast/SWBF2-Lua-API) | GPL-3.0 | Signatures of the script functions the game exposes: the checklist for the host interface |
| [SWBF-msh-Blender-IO](https://github.com/PrismaticFlower/SWBF-msh-Blender-IO) | Apache-2.0 | Model and animation formats |
| [shaderpatch](https://github.com/PrismaticFlower/shaderpatch) | MIT | How the 2005 game's materials and render types are meant to look |
| [SWBF2Phoenix](https://github.com/Ben1138/SWBF2Phoenix) | AGPL-3.0 | A Unity project that ran the retail scripts against its own engine. Design reference for the script host only; no code is copied because of the license difference |
| Community format notes (ze_filetypes and similar) | Mostly unlicensed | Reference only |

Rules:

- Code is taken only from projects whose license is compatible with
  GPL-3.0-only, with attribution in THIRD-PARTY-NOTICES.md.
- Projects that reuse code from the original games are not used, not read
  and not cited, whatever their license.
- Behaviour is checked against the Steam game, not against another project.

## Risks and open questions

1. **Size of the script interface.** A mode calls a few dozen functions, but
   the long tail across all scripts is several hundred. The opening targets
   define the initial set; a script calling a missing function must fail
   loudly in testing and degrade to a logged no-op in a release.
2. **Hero combat.** Combos, blocking and deflection are data in the 2005
   files, but the state machine that plays them is new simulation code that
   the client predicts. It needs its own captures against the Steam game.
3. **Space.** Capital ship interiors on a map with ships outside, hangar
   transitions between flying and walking, and bot pilots are each new. The
   flight model's space setting has only run against synthetic collision.
4. **Bots.** The 2004 AI is organised around command posts. The 2005 modes
   steer bots by goals ("deathmatch", "defend", "destroy"). A goal layer is
   needed before Hero Assault or Space Assault feel alive.
5. **HUD from data.** The 2005 game ships HUD definitions in its files. How
   much of the HUD they describe decides whether the HUD is built from them
   or laid out in code as the 2004 HUD is.
6. **Rendering differences.** Lights as their own chunks, cloth, and the
   2005 material set need a pass through the renderer's program catalog.
7. **Editions.** Whether the Steam, GOG and disc editions of the 2005 game
   agree in gameplay data decides whether they can share servers.
8. **Movies.** 44.1 kHz Bink audio is unverified in the decoder.
