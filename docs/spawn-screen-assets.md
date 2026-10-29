# Spawn screen assets

The extracted data the Classic spawn screen draws: the unit and class select,
the team switch and the spawn post picker. It all comes from the retail
archives and goes to the gitignored `public/` tree. Never commit it.

`tools/zero_spawn_screen.py` holds the pieces, and `tools/extract.py` writes
them with a normal extraction. The same tool also adds them to an existing
extraction without re-extracting it (see [Regenerating](#regenerating)).

## Where everything comes from

| Data | Source | Written to |
| --- | --- | --- |
| Window art: `border_3_pieces` (window background, a 9-slice), `headerbuttonleft`, `headerbuttonmid`, `headerbuttonright` (title bar), `cursor` (title intro sweep block), `opaque_rect` (SPAWN box), all 64×64; `mousecursor` (pointer, 32×32); `shell_cube` (the room's grid, 128×128) | `Common/ENG/common.lvl` `tex_` | `common/textures/<name>.png` and `.ktx2`, `common.json` `textures` |
| The background room, `shell_cube`: one part, 59 vertices, 71 triangles | `common.lvl` `modl` | appended to `common/holograms.bin` and `holograms_index.bin`; record in `common.json` `holograms` |
| 4× lossless WebP atlases of the art above, and of `com_icon_controlzone` (the logos' texture) | the PNGs, through `tools/upscale_hud.py` | `common/raster-hud/`, with `manifest.json` |
| The screen's text | `core.lvl` `Locl`, English | `common.json` `strings` |
| Unit and weapon names | `core.lvl` `Locl` | `sides/<side>/units.json` |
| Camera shots, team sides and objectives | each of the map's mission scripts in `mission.lvl` | `maps/<map>/map.json` `missions[<mission>].spawnScreen` |
| Vehicles at each post | the map's `com_item_vehicle_spawn` objects | `maps/<map>/map.json` `missions[<mission>].spawnScreen` |

The fonts the screen uses (`gamefont_tiny`, `gamefont_medium`,
`gamefont_large`) were already in `common.json` `fonts`. Every font page is
upscaled by `upscale_hud.py`'s page rule: tiny has 3 pages, medium 4 and
large 2.

## Schema

All of it is additions. No existing key changes.

### `public/common/common.json`

- `textures` gains `border_3_pieces`, `headerbuttonleft`, `headerbuttonmid`,
  `headerbuttonright`, `cursor`, `opaque_rect`, `mousecursor` and
  `shell_cube`. Each is a name -> `textures/<name>.png` entry, like the
  existing ones.
- `holograms.shell_cube` is the room, in the same form as the `com_icon_*`
  logos. So `modelPack(common.holograms, holograms.bin, holograms_index.bin)`
  (`src/models.js`) serves it by name, and `partMaterial()` makes its
  material:

  ```json
  [{"texture": "shell_cube", "vertexOffset": 70368, "vertexCount": 59, "indexOffset": 22272,
    "indexCount": 213, "additive": false, "transparent": true, "flags": 1044, "bone": "cube",
    "scroll": [0.0625, 0.0]}]
  ```

  - Flags 0x414: alpha blended, not lit (`partMaterial` makes it unlit and
    double sided), with the texture scrolling 0.0625 per second in u.
  - The vertices are in the model's space. It spans x −5.24..2.68,
    y −1.28..5.12 and z −2.28..2.25, so it is 7.92 × 6.40 × 4.53 units and
    not centred on its origin.
  - The room is packed after the four logos, so their offsets and bytes are
    unchanged.
- `rasterHud` lists the textures `tools/upscale_hud.py` enlarges besides its
  name rule (`hud*`, `*_icon`, `map_mask`, `flag_icon` and the font pages):
  `["border_3_pieces", "headerbuttonleft", "headerbuttonmid",
  "headerbuttonright", "cursor", "opaque_rect", "mousecursor", "shell_cube",
  "com_icon_controlzone"]`.
- `strings` gains `ifs.SpawnDisplay.Spawn`, `game.mapdisplay.objectives`,
  `game.spawndisplay.launchin`, `common.none`, `common.back`,
  `game.tooltips.select_spawn_location`, `game.tooltips.reinforcements`,
  `game.tooltips.select_soldier`, `game.pause.suicide` and
  `game.pause.suicide_prompt`, in English (for example "SPAWN" and
  "OBJECTIVES:").

The atlases land in `public/common/raster-hud/` as
`<name>-<first 12 hex digits of the PNG's sha256>-4x.webp`. `manifest.json`
keys them by the PNG's path in `textures` (`textures/border_3_pieces.png`).
`RasterHudAssets.upgrade()` (`src/raster-hud.js`) finds them for any texture
the HUD loads by its `/common/textures/<name>.png` URL, which is what
`hud.texture(name)` does.

### `public/sides/<side>/units.json`

- `units[class].displayName` is `entity.<side>.<name>`: the class name with
  its first underscore as a dot (`rep_inf_clone_trooper` is
  `entity.rep.inf_clone_trooper`, "CLONE TROOPER").
- Each entry of `units[class].weapons` gains `displayName`:
  `weapons.<side>.weap.<name>`, the class name with its first two
  underscores as dots.
- Both have any trailing line break trimmed. Some retail strings end in
  `\r\n`: the ARC trooper's mines and one Rebel soldier's name, for example.
- The existing `label` is unchanged, line break included.
- A name the language lacks falls back to the class name. Every class and
  loadout weapon of `rep`, `cis`, `imp` and `all` resolves.

### `public/maps/<map>/map.json`: each battle's `spawnScreen`

geo1's `missions.geo1r`, abridged:

```json
"spawnScreen": {
  "cameraShots": [
    {"position": [-6.942698, -59.1972, 26.136919],
     "quaternion": [0.085528, -0.022005, 0.001889, 0.996091]}
  ],
  "teams": {
    "1": {"side": "rep", "objectives": ["CAPTURE ENEMY COMMAND POSTS", "..."]},
    "2": {"side": "cis", "objectives": ["..."]}
  },
  "postVehicles": {
    "cp14": {"1": [], "2": ["SPIDER WALKER", "HAILFIRE TANK"]}
  }
}
```

Each battle the map holds has its own (`missions[<mission>].spawnScreen`; a
battle fought in a world of its own has none). The battle a URL plays puts
its own in `world.spawnScreen` (`src/mission.js`), where the screen reads it.

#### `cameraShots`

- Each shot is a mission's `AddCameraShot(qw, qx, qy, qz, x, y, z)` call, in
  script order. Every stock mission has three.
- The original keeps the call's numbers as they are and builds the
  camera's transform from them (a normalized quaternion turned into a
  rotation matrix). So `position` is in the archive's world frame, the frame
  every other `map.json` transform uses, unconverted.
- `quaternion` is the call's rotation, normalized, in three.js's x, y, z, w
  order.
- The engine's camera looks down its own −z, as a three.js camera does. So a
  shot goes straight onto the camera:
  `camera.position.fromArray(shot.position); camera.quaternion.fromArray(shot.quaternion)`.
  Soldiers face +z instead, which is why the player camera adds π of yaw
  (`src/camera.js`). A shot needs no such turn.
- Checked by placing the game's camera at all 18 shots of the six extracted
  maps (headless SwiftShader). Every one frames a deliberate view: the Theed
  plaza, Echo Base's hangar, the Geonosis war room. Turning by π, or
  mirroring z, points them at walls and sky.

#### `teams`

- Only teams `"1"` and `"2"`, the two the screen offers.
- `side` comes from the team's `SetTeamName`, parsed as the original parses
  it (`zero_ai_mission.faction_of`): Alliance or All is `all`, Empire or Imp
  `imp`, Republic or Rep `rep`, CIS `cis`. Anything else gives `null`. No
  stock mission does that for team 1 or 2.
- `objectives` are the team's `AddMissionObjective(team, colour, key)`
  strings in script order, in English. The colour isn't used on this screen.

#### `postVehicles`

Post name (as in `commandPosts`) -> team -> the vehicles that post's
`com_item_vehicle_spawn` objects give the team. This follows the
original's rules for reading the vehicle-spawn properties and for the spawn
point window:

- **Which post.** A spawn belongs to the command post its `ControlZone`
  names. The name is looked up by hash among the command posts only. So
  it is case-insensitive, and geo1's war room table, a prop
  also named `cp6`, doesn't capture the post's spawns.
- **Which class.** Each team reads its faction's `Class<Faction>`
  (`ClassAlliance`, `ClassEmpire`, `ClassRepublic`, `ClassCIS`,
  `ClassLocals`) for any team, plus `Class<Short>ATK` for team 1 or
  `Class<Short>DEF` for team 2 (`All`, `Imp`, `Rep`, `CIS`, `Loc`).
  - After `SetHistorical`, teams 1 and 2 read `ClassHistorical`, `ClassHisATK`
    and `ClassHisDEF` instead.
  - The last matching property wins.
  - Every stock mission attacks with team 1, so this agrees with
    `zero_vehicle.py`'s attacker-based roles.
- **Listing.** Each class is listed once per post, in world order, by its
  display name (`entity.<prefix>.<rest>`, for example "AT-TE"). A missing
  name falls back to the class name.
- **Which posts appear.** Only posts with at least one tied spawn, in
  `commandPosts` order. Both teams' lists are always present, possibly
  empty. A post absent from `postVehicles` gives nothing ("NONE",
  `common.none`).

Not modelled:

- The original keeps a second class per team and spawn, for one kind of
  spawn class. It names a team-wide class. Which spawn classes set it isn't
  known.

## Regenerating

A normal extraction writes all of it. Run it with a Python that has Pillow
if you want the atlases:

```sh
python3 tools/extract.py geo1 --side rep
```

To add it to an existing extraction, use the incremental mode. It takes
about 16 seconds for everything:

```sh
python3 tools/zero_spawn_screen.py --all-extracted              # every map.json, every units.json, common
python3 tools/zero_spawn_screen.py geo1 nab2 --side rep --common # or just these
```

What the incremental mode rewrites:

- `map.json`: only each battle's `spawnScreen`.
- `units.json`: only the `displayName` fields.
- `common.json`:
  - It adds the textures, the room, the strings and `rasterHud`, and writes
    only the new PNGs.
  - It rebuilds the hologram pack and installs it only if the logos' records
    and bytes come out unchanged. Otherwise it stops and asks for a full
    extraction.
  - It then runs `tools/ktx2.py` on `public/common`, which encodes only the
    new PNGs, and `tools/upscale_hud.py`.

Running it again is harmless.

The atlases need Pillow, the same as `extract_common`. Without it, that step
prints a warning and the game keeps the native art. For example:

```sh
python3 -m venv .venv && .venv/bin/pip install Pillow
.venv/bin/python tools/zero_spawn_screen.py --all-extracted
```

`upscale_hud.py` re-encodes every atlas each run. It reproduces existing ones
byte for byte, and also catches up atlases that a Pillow-less run skipped
(for example `rechargedroid_icon` and `turret_icon`).

Maps with no `map.json` get `spawnScreen` when they are extracted.

## Checks

```sh
python3 -m unittest tests/test_spawn_screen_export.py                    # synthetic, plus an audit of public/ when it has the data
python3 -m unittest discover -s tests -p test_raster_hud.py              # needs Pillow
```

- **Synthetic tests** cover:
  - the shot conversion, including that the quaternion is a three.js
    camera's;
  - names and trimming;
  - team sides and objectives in order;
  - each team's vehicle slot, the historical slots, `ControlZone` lookup
    and de-duplication;
  - the `rasterHud` list in `upscale_hud.py`.
- **The audit** checks `public/` against this schema, including that every
  `rasterHud` texture and every page of `gamefont_tiny`, `gamefont_medium`
  and `gamefont_large` has its atlas.

Run against a snapshot of `public/` (sizes, sha256, JSON key paths), the
incremental mode added 27 files (8 PNGs and their 8 KTX2s, 9 rasterHud
atlases and the 2 catch-up atlases) and changed 15 (`common.json`,
`holograms.bin`, `holograms_index.bin`, the raster HUD manifest,
`common/textures/ktx2.json`, six `map.json` files and four `units.json`
files), all pure additions: no key removed and no value changed, and the two
pack files keep their old bytes as a prefix. Every other file stayed
untouched, including all existing PNGs, KTX2s, atlases, map and side
binaries. A full `tools/extract.py geo1 --side rep` into a scratch folder
produced the same `spawnScreen`, and its common and rep outputs
(`common.json`, `units.json`, the pack, the atlases and the PNGs) matched the
incremental ones byte for byte.
