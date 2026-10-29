# Front end assets

The extracted data the front end draws and plays: the main menu, Instant
Action, the options, the multiplayer screens, the pause menu and the loading
screen. It all comes from the retail archives and goes to the gitignored
`public/` tree. Never commit it.

`tools/zero_shell.py` holds the pieces. `tools/extract.py` writes the
`common.json` part with a normal extraction of common; everything else
(`shell.json`, the movies, the loading screen's pictures and the shell's
sound) comes only from `zero_shell.py` (see [Regenerating](#regenerating)).

## Where everything comes from

| Data | Source | Written to |
| --- | --- | --- |
| Menu art: 17 textures (menu-row bars, the window hover skin, slider parts, `white`, ...) | `Common/ENG/common.lvl` `tex_` | `common/textures/<name>.png` and `.ktx2`, `common.json` `textures` |
| Screen art (22) and pictures (36): Instant Action's backdrop, the multiplayer backdrop, the movies' stills, ... | `Shell/ENG/shell.lvl` `tex_` | the same |
| The loading screen's sprites (18) and tiled patterns (7) | `Load/load.lvl` `tex_` | the same |
| 4× lossless WebP atlases of the 57 sprites | the PNGs, through `tools/upscale_hud.py` | `common/raster-hud/`, with `manifest.json` |
| Every string the shell's and common's scripts use (637), and `common.loading` | `core.lvl` `Locl`, English; keys from the scripts in `shell.lvl` and `common.lvl` | `common.json` `strings` |
| The 55 Bink movies, and each add-on's (Jabba's Palace's flyover) | `Movies/shell.mvs`; an add-on's `Movies/*.mvs`, named by its `Shell/ENG/shell.lvl` | `common/shell/movies/<name>.webm`, with `manifest.json` |
| Each screen's intro and loop, and their fades | the movie config (`shell.lvl` `mcfg`) and the screens' `movieIntro` / `movieBackground` | `common/shell/shell.json` `movies` |
| The Instant Action catalog | `shell.lvl`'s `missionlist` script (`sp_missionselect_listbox_contents`), then what each add-on's `addme.script` appends to it (`tools/zero_addon.py`), the mission scripts in `mission.lvl` and the add-ons', `public/maps/*/map.json` | `shell.json` `maps` |
| The loading screen's config | `Load/load.lvl` `load` chunk | `shell.json` `load` and `maps[].load` |
| The loading screen's pictures (48) | `Load/load.lvl` and `Load/<map>.lvl` | `common/shell/load/<name>.png` |
| The shell music (6 tracks) | `Sound/shell.lvl`, stream `shell_music` (Xbox ADPCM) | `audio/shell/<name>.opus`, `shell.json` `music` |
| The transition whooshes (10) | `Sound/shell.lvl` `SoundLayered`, samples in `Sound/common.bnk` | `audio/shell/shell_transition<N>.flac`, `shell.json` `whooshes` |
| The shell's sound for the audio engine: buses, interface and loading sounds, whooshes | `core.lvl` and `Sound/shell.lvl` configs; `core.lvl`'s bank and `common.bnk` | `audio/shell/sound.json` and `bank.bin` |

All four fonts the menus use (`gamefont_tiny`, `_small`, `_medium` and
`_large`) were already in `common.json` `fonts`, and every page of them has
its 4× atlas through `upscale_hud.py`'s page rule.

## Schema

### `public/common/common.json`

All of it is additions; no existing key changes.

- `textures` gains 100 entries, each `name` → `textures/<name>.png`:

  | Group | Textures |
  | --- | --- |
  | Menu (`common.lvl`) | `buttonleft`, `buttonmid`, `buttonright` (a menu row's bar: 64×64 ends and middle; the right end is a triangle), `border_3a_pieces` (the window skin while the pointer is over it: the 9-slice with an orange border, not a brighter blue), `slider_bg` (navy) and `slider_fg` (orange), 16×16, `white` (32×32), `gray_rect`, `tooltip_bg`, `titlebar_l`, `titlebar_r`, `btna`, `btnb`, `btnmisc`, `btnmisc2` (gamepad hints), `lobby_icons`, `ping_icon` (the player list) |
  | Screens (`shell.lvl`) | `his_brief_bg` (Instant Action's 512×512 backdrop, with the alpha hole the flyover shows through), `arrow_l`, `arrow_r`, `arrow_large`, `lock_icon`, `version_icon` (the session list), `titlebar_large`, `small_title`, `ring_small`, `ring_mask` (the briefing's map ring), `side_a`, `side_c`, `side_i`, `side_r`, `side_alt_a`, `_c`, `_i`, `_r` (Galactic Conquest's side emblems), `cis_own_icon`, `imp_own_icon`, `reb_own_icon`, `rep_own_icon` |
  | Pictures (`shell.lvl`) | `iface_bgmeta_space` (every multiplayer screen's backdrop, 1024²), the stills of the screen movies (`main`, `atat_screen`, `coreship_screen`, `atte_screen`, `hoth_screen`, `generic2_screen`, `generic3_screen`, `gc_screen`, 1024×512; unused by the original, posters here), `movie_bg`, `movie_bg2`, `gc_cfg1`..`8`, `map_bes1`..`map_yav2` and `map_unk` |
  | Loading screen (`load.lvl`) | `load_rotating_disk` (the LOADING disk, 256²), `load_led_0`..`2`, `load_edge_horz`, `load_edge_vert`, `load_edge_corner` (the crosshair's lines and crossing), `load_title_bar_left`, `_middle`, `_right`, `load_side_animation_0`..`5`, `load_planet_backdrop`, `load_planet_rotating_backdrop`, and the tiled `load_noise_0`..`5` and `load_scanline` |

  The team icons the playlist rows show (`rep_icon`, `cis_icon`, `imp_icon`,
  `all_icon`) and the other window and list art were already there.
- `rasterHud` gains 57 entries after the spawn screen's 9 and the
  scoreboard's 10: the menu, screen and loading-screen sprites above, in
  that order (`tools/zero_shell.py` `SHELL_RASTER`). The pictures and the
  tiled patterns stay at their size: a 4× `iface_bgmeta_space` would be
  4096², and enlarging noise smooths it. `his_brief_bg` is enlarged (to
  2048²).
- `strings` gains 602 keys (638 with the 36 already there): every
  localization key a string constant of `shell.lvl`'s or `common.lvl`'s
  scripts names, as the scripts spell it, plus `common.loading` ("LOADING"),
  which the original draws on the loading screen. That includes the menu
  rows, titles, popups and prompts, the options pages, the control
  bindings' names, the multiplayer and profile screens, Galactic Conquest
  and the campaigns, and every planet's and map's name
  (`planets.<planet>.name`, `.mapname1`, `.mapname2`).
  - Text is English, with a trailing line break trimmed. Line breaks inside
    a string stay as the game has them (`\r\n`), for example
    `ifs.controls.General.SquadCommands` is `"SQUAD\r\nCOMMANDS"`.
  - The scripts spell some keys two ways (`ifs.Main.options` and
    `ifs.main.options`); both are there. `common.json` is looked up by the
    exact key.

### `public/common/shell/shell.json`

```json
{
  "movies":   { "<screen>": { "intro": "movies/<name>.webm" | null, "loop": "movies/<name>.webm",
                              "fadeIn": s, "fadeOut": s, "introFadeIn": s, "introFadeOut": s } },
  "maps":     [ { "id", "name", "extracted", "eras": { "cw" | "gcw": { "teams", "attacker", "mission", "extracted" } },
                  "fly", "load": { "planet", "planetKey", "planetTexture", "stages": [ { "image", "target" } ] } } ],
  "music":    [ { "name", "file", "segment", "properties", "duration" } ],
  "whooshes": [ "/audio/shell/shell_transition1.flac", ... ],
  "load":     { ...the loading screen's art, sounds and timings... }
}
```

Paths without a leading slash (`movies/...`, `load/...`) are relative to
`/common/shell/`; the others are absolute URLs: the audio, and the loading
screen's sprites, which are common textures (`/common/textures/<name>.png`).
`RasterHudAssets.upgrade()` finds a sprite's 4× atlas by that path, as it
does for `hud.texture(name)`; a 2D canvas can look it up in
`raster-hud/manifest.json` under `textures/<name>.png`.

#### `movies`

Each shell screen that has a movie, by its script's name. A forward push
plays the screen's intro with its loop queued after it; coming back to the
screen (Esc, BACK) restarts the loop alone. The movie is stretched over the
whole screen.

```json
"ifs_main": {"intro": "movies/main_trans.webm", "loop": "movies/main.webm",
             "fadeIn": 0.0, "fadeOut": 0.5, "introFadeIn": 0.0, "introFadeOut": 0.0}
```

- `fadeIn`, `fadeOut`: the loop's `FadeInTime` and `FadeOutTime` in the
  movie config, in seconds, after what it inherits.
- `introFadeIn`, `introFadeOut`: the intro's. They are only there when
  `intro` is. The config gives the intro and the loop different fades.
- The config's three base sets: intros and one-shots fade in 0 and out
  0.2 s; loops in 0.1 and out 0.5 s; flyovers 1 s each way. The main menu's
  intro and loop override theirs to 0, so `main_trans` runs straight into
  `main`.

| Screen | What it is | Intro | Loop | Fades (loop; intro) |
| --- | --- | --- | --- | --- |
| `ifs_start` | The start screen (on PC it goes straight to the profile screen); the port's "press any key" gate | | `start_screen` (30 s) | 0.1 / 0.5 |
| `ifs_login` | PROFILE SELECTION | | `start_screen` | 0.1 / 0.5 |
| `ifs_main` | MAIN MENU | `main_trans` (1.03 s) | `main` (30 s) | 0 / 0.5; 0 / 0 |
| `ifs_sp` | SINGLEPLAYER | `atat_in` (1.40 s) | `atat_screen` (20 s) | 0.1 / 0.5; 0 / 0.2 |
| `ifs_opt_contmain` | OPTIONS (`ifs_opt_top.lua`) | `coreship_in` (1.50 s) | `coreship_screen` (20 s) | 0.1 / 0.5; 0 / 0.2 |
| `ifs_sp_briefing` | A campaign's briefing | `generic2_in` (1.00 s) | `generic2_screen` (20 s) | 0.1 / 0.5; 1 / 1 |
| `ifs_meta_top`, `ifs_meta_opts` | Galactic Conquest's maps and options | | `generic3_screen` (12 s) | 0.1 / 0.5 |
| `ifs_meta_new_load` | Galactic Conquest, new or load | `generic3_in` (0.83 s) | `generic3_screen` | 0.1 / 0.5; 1 / 1 |
| `ifs_meta_load` | Galactic Conquest's load screen | | `atat_screen` | 0.1 / 0.5 |
| `ifs_meta_main` | The galaxy map | | `gc_screen` (10 s) | 0.1 / 0.5 |
| `ifs_split_profile` | Splitscreen profiles (console only) | `hoth_in` (1.50 s) | `hoth_screen` (20 s) | 0.1 / 0.5; 0 / 0.2 |
| `ifs_tutorials` | Tutorials (hidden on PC) | `atte_in` (1.40 s) | `atte_screen` (20 s) | 0.1 / 0.5; 0 / 0.2 |

Screens without a movie: the options pages keep the options menu's movie
playing behind them; Instant Action draws `his_brief_bg` with the
highlighted map's flyover behind its hole; the multiplayer screens draw
`iface_bgmeta_space`. A flyover fades in and out over 1 s, loops, and starts
0.5 s after the highlighted map changes (the rectangle is in
[shell.md](shell.md)).

The other movies aren't any screen's background. Their files are named the
same way (`movies/<name>.webm`): the campaigns' intros and outros
(`bes1h01`, `bes2h01`, `end1h02`, `end1h03`, `geo1h01`, `hot1h02`,
`kam1h01`, `kam1h02`, `kas2h01`, `nab1h01`, `nab2h01`, `tat1h01`,
`tat2h01`, `yav1h01`, `yav2h01`), as the campaign tables name them, and
Galactic Conquest's endings (`rebel_win`, `rebel_base`, `cis_win`,
`cis_base`, `imperial_win`, `imperial_base`, `republic_win`,
`republic_base`). These have sound.

#### `maps`

Every map Instant Action lists, in its order (BES1 to YAV2).

```json
{
  "id": "nab2",
  "name": "NABOO: THEED",
  "extracted": true,
  "eras": {
    "cw":  {"teams": ["cis", "rep"], "attacker": 1, "mission": "nab2c", "extracted": true},
    "gcw": {"teams": ["all", "imp"], "attacker": 1, "mission": "nab2a", "extracted": false}
  },
  "fly": "movies/nab2fly.webm",
  "load": {
    "planet": "NABOO", "planetKey": "planets.naboo.name", "planetTexture": "load/load_planet_naboo.png",
    "stages": [
      {"image": "load/load_galacticmap_0.png", "target": [350, 658, 37, 43]},
      {"image": "load/load_locale_naboo_1.png", "target": [348, 514, 48, 50]},
      {"image": "load/load_landsat_naboo_2.png", "target": [212, 517, 37, 43]},
      {"image": "load/load_level_naboo2_3.png", "target": null}
    ]
  }
}
```

- `id`: what `?map=` takes, the map archive's name (`public/maps/<id>/`).
- `name`: the list's text (`planets.<planet>.mapname1` or `2`). An
  add-on's entry names it as written ("TATOOINE: JABBA"), which the shell
  shows as is when localization has nothing for it.
- `extracted`: `public/maps/<id>/map.json` exists, so the map is playable
  here. Maps that aren't are dimmed and can't be added.
- `eras`: the eras the list offers, from its `side_<letter>` flags. A map
  with both gets the era picker.
  - `teams`: team 1's side and team 2's (`rep`, `cis`, `all`, `imp`), from
    the mission's `SetTeamName` calls as the original parses them.
  - `attacker`: the team that attacks, from `SetAttackingTeam`. It is 1 in
    every era: the side letter names team 1, and the player starts on it.
    `kas1i` computes its argument; there the letter decides. Jabba's
    Palace's `tat3c` is the exception: its letter is the CIS's, but its
    script puts the Republic on team 1 and has team 1 attack.
  - `mission`: the original's playlist entry, the map and the side letter
    (`r` Republic, `c` CIS, `a` Alliance, `i` Empire attack), which is also
    the mission script's name.
  - `extracted`: the extraction holds this era's mission (map.json's
    `missions`).
- `fly`: the flyover. All 16 exist; Naboo: Theed's is 640×480, the rest
  720×480. Jabba's Palace's (`tat3fly`, 720×480) is in its add-on's
  `Movies/tat3.mvs`.
- `load`: the loading screen's zoom for this map (below). `null` for Jabba's
  Palace: `load.lvl` has no `World` for `tat3` and the add-on brings no
  pictures, so its loading screen is the planet's name and the disk over
  the plain backdrop.

| Map | Name | Clone Wars | Galactic Civil War | Historical versions |
| --- | --- | --- | --- | --- |
| bes1 | BESPIN: PLATFORMS | bes1r | bes1a | bes1a_h |
| bes2 | BESPIN: CLOUD CITY | bes2r | bes2a | bes2a_h |
| end1 | ENDOR: BUNKER | | end1a | end1a_h |
| geo1 | GEONOSIS: SPIRE | geo1r | | geo1r_h |
| hot1 | HOTH: ECHO BASE | | hot1i | hot1i_h |
| kam1 | KAMINO: TIPOCA CITY | kam1c | | kam1c_h |
| kas1 | KASHYYYK: ISLANDS | kas1c | kas1i | kas1c_h |
| kas2 | KASHYYYK: DOCKS | kas2c | kas2i | kas2c_h |
| nab1 | NABOO: PLAINS | nab1c | nab1i | nab1c_h (in `NAB/nab1_h.lvl`) |
| nab2 | NABOO: THEED | nab2c | nab2a | nab2c_h (in `NAB/nab2_h.lvl`) |
| rhn1 | RHEN VAR: HARBOR | rhn1r | rhn1i | rhn1r_h |
| rhn2 | RHEN VAR: CITADEL | rhn2c | rhn2a | rhn2a_h |
| tat1 | TATOOINE: DUNE SEA | tat1r | tat1i | tat1i_h |
| tat2 | TATOOINE: MOS EISLEY | tat2r | tat2i | tat2i_h |
| yav1 | YAVIN 4: TEMPLE | yav1c | yav1i | yav1i_h |
| yav2 | YAVIN 4: ARENA | yav2r | yav2i | yav2i_h |
| tat3 | TATOOINE: JABBA (add-on, `AddOn/Tat3`) | tat3c | tat3a | |

**Eras and `?map=`.** A map's extraction holds every battle fought in its
world (map.json's `missions`, by mission name), so each era of every map is
`extracted` once the map is. `?map=<id>&era=<era>` plays that era's battle;
`?map=<id>` alone plays its Clone Wars battle, or the one it has
(`src/mission.js`). The historical versions are imported and marked
(`historical`) for a campaign mode to come; Instant Action and servers never
pick them. The campaign's Naboo battles are fought in worlds of their own
(`NAB/nab1_h.lvl` and `nab2_h.lvl`, which no map extracts yet): those two are
recorded with their `world` and never played on the regular map's.

#### `music`

The six segments of the `shell_music` stream, as Ogg Opus (48 kHz stereo,
256 kb/s, decoded from the Xbox ADPCM):

```json
{"name": "shell_clonewars", "file": "/audio/shell/shell_clonewars.opus", "segment": "0xc27a5daa",
 "properties": ["shell_soundtrack", "shell_clonewars"], "duration": 65.387}
```

| # | Name | Length | Played by |
| --- | --- | --- | --- |
| 1 | `shell_soundtrack_1` | 52.8 s | `shell_soundtrack` |
| 2 | `shell_clonewars` | 65.4 s | `shell_soundtrack`, `shell_clonewars` |
| 3 | `shell_clash` | 68.0 s | `shell_soundtrack`, `shell_clash` |
| 4 | `shell_imperialmarch` | 183.7 s | `shell_soundtrack`, `shell_imperialmarch` |
| 5 | `shell_soundtrack_5` | 133.7 s | `shell_soundtrack` |
| 6 | `shell_droidinvasion` | 52.5 s | `shell_galacticconquest`, `shell_droidinvasion` |

- The order is `shell_soundtrack`'s list, then the rest. The segments'
  own names aren't known, so each is named for the most particular
  property that plays it.
- `segment` is the stream's segment id; `properties` the
  `SoundStreamProperties` that play it, so a screen's `music` finds its
  track; `duration` is in seconds.
- How the original plays it: `shell_soundtrack` starts with
  the profile screen and cycles the first five (CyclePlayback: in order,
  from a random first pick as `src/audio/props.js` does it), 10 s of silence
  between tracks (0 while the audio options page is open). Galactic
  Conquest switches to `shell_galacticconquest`, each briefing to its
  mission's track. The bus is `shellmusic` (gain 0.6, under `music`, `main`
  and `ingame`). A forward transition fades `shellmusic` to 0 over 1 s, and
  back to 1 over 1 s once the intro has played.

#### `whooshes`

`shell_transition1` to `10`, in order: a forward transition with an intro
plays the next one, wrapping after 10. Each is FLAC, 44.1 kHz stereo:

- The original is a `SoundLayered` of two mono samples (one channel each of
  a stereo whoosh): a layer at pitch 0.99 panned hard left and one at pitch
  1 panned hard right, both at gain 1, on the `shellfx` bus (gain 0.3,
  directly under `root`).
- The file has each layer on its side, at its pitch, both starting at 0.
  What the original rolls each play isn't baked in: each layer's pitch
  ±0.1 and its start 0 to 0.5 s late.
- The original game never gives a 2D voice its `Pan`, so the original mode
  plays both layers centred. To have that, or the rolls, play
  `shell_transition<N>` from `/audio/shell/sound.json` through the audio
  engine (below), which does both.

#### `load`

The loading screen, from `load.lvl`'s config (abridged; every sprite is a
`/common/textures/` URL):

```json
{
  "backdrop": "load/load_simple_backdrop.png",
  "disk": "/common/textures/load_rotating_disk.png", "text": "common.loading",
  "leds": ["/common/textures/load_led_0.png", "/common/textures/load_led_1.png", "/common/textures/load_led_2.png"],
  "edges": {"horizontal": "/common/textures/load_edge_horz.png", "vertical": "/common/textures/load_edge_vert.png",
            "corner": "/common/textures/load_edge_corner.png"},
  "titleBar": ["/common/textures/load_title_bar_left.png", "...middle", "...right"],
  "sideAnimation": {"textures": ["/common/textures/load_side_animation_0.png", "...5"], "args": [6]},
  "noise": {"textures": ["/common/textures/load_noise_0.png", "...5"], "args": [30, 6, 6]},
  "scanline": {"texture": "/common/textures/load_scanline.png", "args": [1, 12, 0.5]},
  "planetBackdrops": ["/common/textures/load_planet_backdrop.png", "/common/textures/load_planet_rotating_backdrop.png"],
  "teams": ["all", "imp", "rep", "cis"],
  "light": {"ambient": [0, 0, 0], "color": [255, 255, 255], "direction": [0.705, -0.078, -0.705]},
  "timing": [3.5, 1],
  "sounds": {"xTracking": "load_xtracking", "yTracking": "load_ytracking", "zoom": "load_zoom",
             "transition": "load_transition", "bar": "load_bar"},
  "barSoundInterval": 1
}
```

- `backdrop` is the "simple" loading screen (1024², the logo on the blue
  grid) the original shows going back to the shell.
- `disk` and `text`: the LOADING disk (bottom right) and its word's key.
- `edges`: the crosshair, the cyan tracking lines (32×32 sprites: a
  horizontal line, a vertical one, their crossing).
- `sideAnimation`, `noise`: frames named by the config's prefix and count;
  `args` are the config's remaining numbers (likely rates and sizes; not
  confirmed). `scanline.args` likewise.
- `teams`, `light`: the config's team models and the rotating planet's
  light.
- `timing`: the config's `TargetTime` (3.5, 1): the seconds each picture's
  crosshair tracks (the horizontal pair in the first half, the vertical in
  the second) and the zoom that follows, as the original reads them; it
  falls back to 2 and 0.5 without one
  ([shell.md](shell.md)).
- `sounds`: `core.lvl` sound names; all are in `/audio/shell/sound.json`
  and every battle's. `load_transition`'s sample isn't in the install.
  `load_xtracking` and `load_ytracking` are looping sounds on one 0.35 s
  sample (7056 Hz) at pitch 2.1 and 1.8, gain 0.7; `load_zoom` is a 0.43 s
  one-shot at pitch 1.5; `load_bar` loops the tracking sample at pitch 0.4
  with gain 0, so it is silent. All sit on the `shellfx` bus. The
  original plays each tracking loop while its pair of lines moves, the
  zoom as a zoom starts and the transition as it ends; the port plays the
  same ([shell.md](shell.md)).
- The config's key names were found by hashing candidates; the edges' key
  (`0x6ae7b95f`) wasn't, and is called `EdgeTextures` here. The config also
  names a `load_logo`, which the install doesn't have.

Each map's `load`:

- `planet`: the name drawn top left, in English (`planetKey` is its key).
- `planetTexture`: the rotating planet's texture (256×128). The planet model
  itself isn't extracted (a sphere).
- `stages`: four pictures, each filling the screen in turn: the galaxy
  (shared by every map), the planet in space, its surface (shared by a
  planet's two maps) and the level (1024×512). `target` is `[x, y, w, h]`
  in pixels of that stage's picture, from its top left: the crosshair
  tracks onto it, then the view zooms into it and the next stage takes
  over. The last stage has none. The picture is stretched over the screen:
  in the captures of the original the crosshair's horizontal pair sits
  exactly on geo1's galaxy target (y 737 and 780, times 768/1024), while the
  vertical pair is still moving.

### `public/common/shell/movies/`

The 55 movies of `shell.mvs` as WebM: VP9 profile 0, 4:2:0, tagged BT.601
limited range (what the Bink decoder outputs; checked against a
capture of the main menu, and Chromium decodes them so), one pass at
constant quality, every source frame kept with its timing, a keyframe at
least every 10 s, nothing faded or trimmed: a loop's last frame runs into
its first as in the original. Sound, where a movie has it, is Opus 160
kb/s, 48 kHz stereo. Only the campaign and Galactic Conquest movies have
sound.

| Kind | Movies | Size | Encoding |
| --- | --- | --- | --- |
| Main screens' backdrops | `start_screen`, `main_trans`, `main`, `atat_in`, `atat_screen`, `coreship_in`, `coreship_screen` | 800×600 | CRF 18, `cpu-used` 6 |
| Other screens' backdrops | `generic2_*`, `generic3_*`, `gc_screen`, `hoth_*`, `atte_*` | 800×600 | CRF 20, `cpu-used` 6 |
| Flyovers | `<map>fly` × 16 | 720×480 (`nab2fly` 640×480) | CRF 20, `cpu-used` 6 |
| Campaign and Galactic Conquest movies | 15 + 8 | 800×600 | CRF 22, `cpu-used` 6 |

Every movie is one VP9 pass at its source size. The main backdrops were
once stored at 3× (Lanczos and a luma unsharp mask) to look sharper; side by
side with the source that looked the same, since resampling cannot recover
detail the 800×600 frames lack, and it cost most of the desktop app's
first-launch encode. The shell now draws each movie through
`src/shell/video-sharpen.js`: a Catmull-Rom upscale to the screen pixels it
covers, then FSR 1's RCAS contrast-adaptive sharpening (strength 0.7;
`?moviesharpen=0` to `1` overrides it, 0 shows the plain video). Without
WebGL2 the plain `<video>` plays. The player contains the image instead of
cropping and enlarging it on wide displays.

`manifest.json` lists each movie (this example is from the earlier 2× import):

```json
"main": {"file": "main.webm", "hash": "0xea90e208", "kind": "backdrop", "width": 1600, "height": 1200,
         "frames": 900, "fps": [30, 1], "duration": 30.0, "audio": false, "bytes": 4954318,
         "settings": "...the ffmpeg arguments...", "source": {"offset": 18630656, "length": 18006528, "width": 800, "height": 600}}
```

How `shell.mvs` is laid out: a ucfb tree whose tags are name hashes, like
the sound archives. An `Info` chunk names the container (`shell`) and lists
each movie in file order (`Segment`: `Name` hash, `Length`, `Padding`); a
`Data` chunk holds the Bink files back to back, each followed by its
padding. 41 names are in the scripts or textures (or are `<map>fly`); `main_trans`,
`atat_in`, `coreship_in`, `generic2_in`, `generic3_in` and `hoth_in` were
found by hashing candidates; Galactic Conquest's eight endings are named
for the `MovieProperties` that play them (`Rebel_win`...), since their
segments' names aren't known. The config's `0xa2672d26` names a movie the
archive doesn't have.

### `public/common/shell/load/`

48 PNGs (RGB, the pictures are DXT1): `load_simple_backdrop`, the galaxy,
10 planets' locale and landsat pictures and planet textures, and 16 level
pictures. Every `Load/<map>.lvl` repeats the galaxy, and a planet's two
maps repeat its pictures; the copies are byte-identical, so each is written
once.

### `public/audio/shell/`

- The six music tracks (`<name>.opus`) and ten whooshes
  (`shell_transition<N>.flac`) above.
- `sound.json` and `bank.bin`: a sound sub-level named `shell`, in the form
  `tools/zero_sound.py` writes a battle's, so
  `SoundData.load("/audio", "shell")` (`src/audio/data.js`) loads it and
  the audio engine plays its sounds by name: `core.lvl`'s objects (the
  buses, `shell_select_change`, `shell_menu_accept`, `_enter`, `_cancel`,
  `_error`, `_exit`, `shell_keyboard_key`, the `load_*` sounds) and
  `Sound/shell.lvl`'s (the music's and voice-over's properties and the ten
  `shell_transition<N>`). `bank.bin` holds the 26 samples they use (16-bit
  PCM, mono): `core.lvl`'s six and the whooshes' twenty. It lists no
  streams; the music plays from its own files. Without it the shell has no
  interface sounds: they live in `core.lvl`, which today only comes inside
  each battle's sound.

## Regenerating

Everything (the movies take hours on a busy machine; a movie already
encoded with the same settings is skipped):

```sh
.venv/bin/python tools/zero_shell.py              # a venv with Pillow for the atlases; needs ffmpeg
```

Or a part at a time:

```sh
.venv/bin/python tools/zero_shell.py --common     # common.json, PNGs, KTX2 copies, atlases (~6 min)
python3 tools/zero_shell.py --audio               # music, whooshes, sound.json; then shell.json (~7 min)
python3 tools/zero_shell.py --catalog             # shell.json and the loading pictures (~2 min)
python3 tools/zero_shell.py --movies --jobs 2     # the transcodes; --only main kam1fly; --force
```

- `--common` adds the textures, strings and `rasterHud` entries and writes
  only the new PNGs, then runs `tools/ktx2.py` on `public/common` and
  `tools/upscale_hud.py`, as the spawn screen and scoreboard tools do. A
  full `tools/extract.py` run writes the same entries
  (`extract_common`); a sandboxed run produced the same strings,
  `rasterHud`, PNG bytes and atlases.
- `--catalog` rereads `public/maps/*/map.json` for the `extracted` flags:
  run it after extracting a map. It keeps `music` and `whooshes`.
- `--audio` rewrites `music` and `whooshes` and the catalog with them. It
  keeps existing `.opus` and `.flac` files; delete them to re-encode.
- `--movies` keeps `movies/manifest.json` current as each movie lands, and
  works through the main screens first, then the flyovers, the other
  backdrops and the story movies. Each encode uses four threads; `--jobs`
  runs several at once. The Bink is copied to a temporary folder and
  removed after encoding.

Running any part again is harmless.

## Sizes

| Output | Files | Size |
| --- | --- | --- |
| `common/textures/` additions | 100 PNG + 100 KTX2 | 7.2 MB + 5.5 MB |
| `common/raster-hud/` additions | 57 atlases | 5.2 MB |
| `common/shell/load/` | 48 PNG | 23 MB |
| `common/shell/movies/` | 55 WebM + manifest | each movie's `bytes` in `manifest.json` |
| `audio/shell/` | 6 Opus, 10 FLAC, `sound.json`, `bank.bin` | 17.4 + 2.9 + 5.1 MB |

## Checks

```sh
python3 -m unittest tests/test_shell_export.py   # synthetic, plus an audit of public/ when it has the data
```

- **Synthetic tests**: the movie archive's index and padding, Bink headers,
  the movie config's fades and inheritance, the movies' names, the screens'
  movies, the catalog's eras, teams, missions and flags, the loading
  screen's config, the music's order and names, the PNG writer, and the
  texture and raster lists.
- **The audit** checks `public/` against this schema: every texture's PNG;
  `rasterHud`'s order (spawn screen, scoreboard, front end) and an atlas for
  each entry and for every page of the four fonts; the strings; every
  screen's and map's movie; each map's eras against its `map.json`; every
  loading picture and sprite; the music and whooshes; and, with ffprobe,
  every movie as VP9 at its manifest's size (with Opus where it has sound)
  and every audio file's codec and length.
- `tests/test_scoreboard_export.py`'s audit now checks only its own slice of
  `rasterHud`.

Headless Chromium plays the WebMs at their size, and its pixels match a
BT.601 limited-range decode of the same frames (mean error under 0.2 of 255;
BT.709 would be off by 1 to 6). A loop's seam is as small as the source's:
`main.webm` keeps all 900 frames (30.000 s), and its step from the last frame
to the first is 41.6 dB PSNR against 42.0 dB for the same frames of the Bink,
upscaled the same way. The encode is 42 to 50 dB from the source at those
frames.
