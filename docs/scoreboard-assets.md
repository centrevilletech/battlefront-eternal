# Scoreboard assets

The extracted data the Classic scoreboard draws: the end-of-match table
(and Tab's table, built from it), DETAILS, AWARDS and the stat popup. It
all comes from the retail archives and goes to the gitignored `public/`
tree. Never commit it.

`tools/zero_scoreboard.py` holds the lists, and `tools/extract.py` writes
them with a normal extraction of common. The same tool also adds them to an
existing extraction without re-extracting it (see
[Regenerating](#regenerating)).

## Where everything comes from

| Data | Source | Written to |
| --- | --- | --- |
| List art: `opaque_black` (16×16, black at alpha 119, the dimming), `listbox_cursor` (the selection's frame, 64×64, nine-sliced), `listbox_hilight` (the hovered row's), `small_arrow` (64×32, turned to point up or down), `backbutton_left`, `backbutton_mid`, `backbutton_right` (DETAILS' tab; 32×32, 64×32, 32×32) | `Common/ENG/common.lvl` `tex_` | `common/textures/<name>.png` and `.ktx2`, `common.json` `textures` |
| 4× lossless WebP atlases of the art above, and of `stats_kills`, `stats_deaths` and `stats_cpp` (the column icons and the popup's) | the PNGs, through `tools/upscale_hud.py` | `common/raster-hud/`, with `manifest.json` |
| The text of the table, DETAILS and AWARDS | `core.lvl` `Locl`, English | `common.json` `strings` |
| Bot name lists | `core.lvl` `Locl` (`common.sides.<side>.firstnames.NN`, `.lastnames.NN`, `level.<map>.local`) | `maps/<map>/ai-mission.json` `names` (tools/zero_ai_mission.py, unchanged) |

**Already in common, from the HUD and the spawn screen:**

- The stat icons, `stats_kills`, `stats_deaths` and `stats_cpp` (64×64,
  extract.py `HUD_TEXTURES`). They now go into `rasterHud`, since
  `upscale_hud.py`'s name rule (`hud*`, `*_icon`, `map_mask`, `flag_icon`)
  misses them.
- `border_3_pieces` (the window), `cursor` (the intro's block) and
  `mousecursor` (the pointer), from the spawn screen.
- The logos: `com_icon_republic`, `_cis`, `_imperial` and `_alliance`
  in the hologram pack, and their texture `com_icon_controlzone`.

**Fonts:**

- `gamefont_medium` draws the table, its titles, DONE, AWARDS, BACK, and
  AWARDS' rows.
- `gamefont_small` draws DETAILS' label, DETAILS' rows and the popup's
  numbers.
- Both are in `common.json` `fonts`, and every page of them is upscaled by
  `upscale_hud.py`'s page rule: small has 3 pages, medium 4.

**Not used, though the original has them:**

- `slider_bg`, `slider_fg` and `ping_icon`: the network list's.
- `btna` and `btnb`: mouse-button help.
- `lobby_icons`: Xbox Live.
- `iface_bgmeta_space`: the shell's background.
- `gamepad_xbox360`: gamepad icons.

## Schema

All of it is additions. No existing key changes.

### `public/common/common.json`

- `textures` gains `opaque_black`, `listbox_cursor`, `listbox_hilight`,
  `small_arrow`, `backbutton_left`, `backbutton_mid` and
  `backbutton_right`. Each is a name → `textures/<name>.png` entry, like
  the existing ones.
- `rasterHud` gains, after the spawn screen's nine entries:

  ```json
  ["opaque_black", "listbox_cursor", "listbox_hilight", "small_arrow",
   "backbutton_left", "backbutton_mid", "backbutton_right",
   "stats_kills", "stats_deaths", "stats_cpp"]
  ```

- `strings` gains these keys, in English (read from the player's own
  `core.lvl` at extraction):

  | Keys | What they label |
  | --- | --- |
  | `ifs.Stats.kills`, `ifs.stats.deaths`, `ifs.stats.commandpostpoints` | the three column headings |
  | `ifs.stats.done`, `ifs.Stats.details`, `ifs.Stats.awards` | the DONE, DETAILS and AWARDS buttons |
  | `ifs.controls.playerlist`, `game.pause.playerlist` | the player list |
  | `ifs.stats.personalstatstitle` | DETAILS' title |
  | `ifs.stats.headshots`, `.favclass`, `.favweapon`, `.favvehicle` | DETAILS' first four rows |
  | `ifs.stats.livingstreak`, `.narrowescapes`, `.vehiclesdestroyed` | its next three rows |
  | `ifs.stats.nemesis`, `.bait` | its last two rows |
  | `ifs.stats.awardstatstitle` | AWARDS' title |
  | `ifs.stats.tankbuster`, `.killingspree`, `.camper`, `.survivalist` | four of the eight awards |
  | `ifs.stats.traitor`, `.publicenemy`, `.meat`, `.deadeye` | the other four |

  - The keys are spelled as the Lua spells them.
    Localization hashes keys without regard to case, but `common.json` is
    looked up by the exact key.
  - BANTHA FODDER's key is `ifs.stats.meat`.
  - The game asks for the award and DETAILS labels by hash; these keys
    hash to the same values.
  - `common.back` (BACK) and `common.sides.*.name` (the team titles) were
    already there.

The atlases land in `public/common/raster-hud/` as
`<name>-<first 12 hex digits of the PNG's sha256>-4x.webp`. `manifest.json`
keys them by the PNG's path in `textures`. `RasterHudAssets.upgrade()`
(`src/raster-hud.js`) finds them for any texture the HUD loads by its
`/common/textures/<name>.png` URL, which is what `hud.texture(name)` does:
everything the board and the popup draw goes through it.

### `public/maps/<map>/ai-mission.json`: `names`

Unchanged. The Classic AI and `Battle.nameBots()` read it, through
`src/bot-names.js`:

- `"1"` to `"4"`: `{ first, last }` for the Rebels, Empire, Republic and
  CIS. Empty lists mean two letters, or three digits.
- `"0"`: the neutral side's.
- `local`: the level's local name.

## Regenerating

A normal extraction writes all of it:

```sh
python3 tools/extract.py geo1 --side rep
```

To add it to an existing extraction, use the incremental mode. It takes
about 25 seconds:

```sh
.venv/bin/python tools/zero_scoreboard.py
```

**It rewrites only `common.json`:**

- It adds the textures, strings and `rasterHud` entries, and writes only
  the new PNGs.
- It then runs `tools/ktx2.py` on `public/common`, which encodes only the
  new PNGs, and `tools/upscale_hud.py`, which writes the new atlases and
  reproduces the existing ones byte for byte.

Running it again is harmless. The atlases need Pillow. Without it that
step prints a warning, and the game keeps the native art.

Run against a snapshot of `public/common`, it added 24 files (7 PNGs and
their 7 KTX2s, and 10 atlases: the 7 new textures and the 3 stat icons) and
changed 3 (`common.json`, `raster-hud/manifest.json` and
`textures/ktx2.json`), every change a pure addition: no key removed and no
value changed, with `rasterHud` keeping the spawn screen's entries first.
Every other file stayed untouched.

## Checks

```sh
python3 -m unittest tests/test_scoreboard_export.py   # synthetic, plus an audit of public/ when it has the data
```

- **The synthetic tests** cover the texture, raster and string lists, and
  that strings are trimmed and only present ones kept.
- **The audit** checks `public/` against this schema:
  - every texture's PNG;
  - the order of `rasterHud`;
  - every string;
  - an atlas for every texture the board and popup draw, and for every
    page of `gamefont_small` and `gamefont_medium`;
  - the four logos in the hologram pack.
- `tests/test_spawn_screen_export.py`'s audit now checks only that its own
  `rasterHud` entries come first.
