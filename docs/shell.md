# The front end

The original's front end ("the shell"): the main menu, SINGLEPLAYER and
Instant Action with its playlist, OPTIONS, the multiplayer and profile
screens, over the original's background movies and music, and the
loading screen between it and a battle. The look is the original's; the
content is the port's (its settings, its maps).

It is built in two parts:

- **The media side and the plumbing** (this document): the battles a
  playlist launches, the movies, the music, the click gate and the loading
  screen.
- **The screens** (`rust/bfe-shell`, on the menu framework of
  [menus.md](menus.md)): MAIN MENU, SINGLEPLAYER, Instant Action, OPTIONS,
  the multiplayer and profile screens, EXIT TO WINDOWS.

The original's behaviour follows its interface Lua (`ifs_main`,
`ifs_sp`, `ifs_missionselect`, `ifs_movietrans`, `ifelem_shellscreen`),
its movie config (shell.lvl `mcfg`) and Load/load.lvl's loading config,
and was checked against captures of the 2004 game. The data comes from
`tools/zero_shell.py` ([shell-assets.md](shell-assets.md)).

The profile is the player's name (the `playerName` setting), which a
server shows to the other players. Multiplayer needs one: joining a
server (from the LAN or ONLINE list, or a direct connection) or
hosting one first asks for a name while the name is still the default
PLAYER 1, which DELETE PROFILE also returns to. Cancelling launches
nothing.

## Files

| Crate, module | What it does |
| --- | --- |
| `rust/bfe-shell` (`screens`) | `ShellApp`: the screen stack from the gate to the main menu, SINGLEPLAYER, Instant Action, OPTIONS, the profile and multiplayer screens |
| `rust/bfe-shell` (`launch`) | The Instant Action playlist and `BattleRequest`: LAUNCH, the next battle, RESTART MISSION |
| `rust/bfe-shell` (`version`) | The main menu's version line, bottom right: the build's preview number, the simulation (`bfe-sim-N`, which servers must match) and the game data's map, side and common generations (`M12 S2 C2`), with what the build needs when the data is stale |
| `rust/bfe-shell` (`data`) | `common/shell/shell.json` and `common.json`, read with fallbacks for whatever is missing |
| `rust/bfe-shell` (`media`, `movies`, `movie_gpu`) | Each screen's movie and music, the moves between screens (`ifs_movietrans`), Instant Action's preview; the movie layers and their fades; the frames' upload, colour conversion, upscale and sharpening |
| `rust/bfe-shell` (`music`, `audio`) | The tracks in turn and the gaps; the front end's mixer (the buses, the dips, whooshes, menu and loading sounds) |
| `rust/bfe-shell` (`gate`) | The click gate's text, layout and pulse |
| `rust/bfe-shell` (`loading`) | The loading screen: the original's timeline and its sounds, the disk, the lights, the zoom |
| `rust/bfe-shell` (`profile`) | The profile rules |
| `rust/bfe-shell` (`page`, `render`) | Layout and drawing of the front end's screens, the server browser and the battle screens' look |
| `rust/bfe-video` | The Bink movies, decoded from the game's own files |
| `rust/bfe-client` (`app/shell.rs`) | The native host of the front end: textures, the screens' state, launching battles and joining servers |
| `rust/bfe-client` (`app/page_backdrop.rs`) | The front end's backdrop: a movie down each side, under every screen |
| `rust/bfe-client` (`app/loading.rs`) | The battle's load on a background thread while the window draws the loading screen |
| `rust/bfe-ui` | The Slint screens of the front end's own look |

## Battles and the playlist

A battle is a `BattleRequest` (`rust/bfe-shell` `launch`): a map and era,
the Instant Action playlist it is part of, and its place in it.

| Field | What it is |
| --- | --- |
| map | This battle's map (its folder in the prepared data) |
| era | Its era, `cw` or `gcw`: the map's battle of that era. A map without that era plays the one it has |
| playlist | The playlist, `map:era` entries, in the order they play this lap; 64 at most, as the original's mission queue |
| round | This battle's entry in it, from 0 |
| random | Shuffle again at each wrap (RANDOM ORDER), or PLAY IN ORDER |

| Function | Gives |
| --- | --- |
| `BattleRequest::launch` | LAUNCH: the playlist's first battle; shuffled first in random order |
| `BattleRequest::next` | After a battle: `round + 1`; past the last, back to 0, shuffled again in random order; without a playlist, the same battle |
| `BattleRequest::restart` | The same battle (RESTART MISSION) |
| `parse_rotation`, `format_rotation`, `shuffle`, `MAX_PLAYLIST` | The playlist's text, the original's shuffle (7 x N swaps of two random entries), 64 |

Which of a map's battles plays follows one rule, the same in the client's
world thread and on a server: a named mission, else the era, else the era
of the chosen side (`rep` and `cis` fight the Clone Wars, `all` and `imp`
the Galactic Civil War), else the map's Clone Wars battle, else its other.
Never a historical version unless named.

## Instant Action

SINGLEPLAYER opens Instant Action, which builds the playlist: AVAILABLE MAPS
on one side, SELECTED MAPS on the other. Enter or a double click on an
available map adds it (an era picker opens when the map has two eras to
choose from) and on a selected one removes it; ADD and REMOVE do the same
with the pointer, and Delete removes the selected entry. Maps that are not
extracted stay in the list, dimmed, and cannot be added. The playlist holds
64 entries at most. PLAY IN ORDER / RANDOM ORDER sets the order, and
LAUNCH, unavailable while the playlist is empty, opens the playlist's first
battle.

## After a battle

The original's DONE (or 30 s without input on the end-of-match table)
calls `ScriptCB_QuitFromStats`, which loads the next mission of the queue;
Instant Action's queue always loops, so it never runs out, and in RANDOM
ORDER it is shuffled again at every wrap. Here the scoreboard's DONE and
timeout load the playlist's next entry (`BattleRequest::next`), or, for a
battle opened without a playlist, the same battle again.

The only way back to the front end is the pause menu's QUIT
([menus.md](menus.md)). The original goes back to the
MAIN MENU, not to the Instant Action screen, and so does the port.

## The front end

- **The click gate.** The front end opens on the original's first screen
  movie (`start_screen`, the profile screen's flythrough) with **PRESS ANY
  KEY OR CLICK TO BEGIN** (the port's own words: the PC game has no such
  line) pulsing at the bottom, as a focused button's label does (alpha 1 to
  0.3 and back in 0.7 s), in `gamefont_medium`. The press starts the
  soundtrack, then moves on to the main menu with its intro and the initial
  whoosh, as the original's profile OK does. QUIT from a battle goes
  straight to the main menu with its intro.
- **The front end's own look** (a design in progress, drawn by Slint,
  [menus.md](menus.md)) draws the screens, popups and dialogs over one
  backdrop, instead of the original's menus over each screen's movie. One
  movie fades in down the left (the Clone Wars: `republic_win`,
  `republic_base`, `cis_win`) and one down the right (the Galactic Civil
  War: `imperial_win`, `rebel_base`, `rebel_win`), each crossing into the
  next, sharpened as the other backgrounds are (`app/page_backdrop.rs`); a
  movie that is missing is passed over. They play on under every screen, so
  no screen plays a movie of its own. The gate shows the Battlefront
  Eternal logo (drawn in Saira, SIL OFL) over PRESS ANY KEY OR CLICK TO
  BEGIN; the main menu's rows come in under the logo; every other screen
  has its title at the top, a small logo in the corner and its BACK, middle
  and accept buttons along the bottom, their outer edges on the content's
  (a wide screen's panels, an option page's panel). Instant Action shows
  its lists either side of the highlighted map's flyover, in a frame of its
  own, the available maps under the game they're from (BATTLEFRONT (2004);
  the 2005 game's are listed as planned). Option pages, Instant Action and
  the server browser darken the backdrop behind them. MULTIPLAYER is the
  server browser, all on one screen: JOIN, CREATE (the hosting dialog) and
  LEADERBOARD (planned) tabs; INTERNET or LAN; a search, map, era and ping
  filters, hide full or empty; the servers under headers that sort them;
  and GAME INFO for the one picked: its map's flyover, mode, era, bots and
  teams, players, ping, place (a flag where one is drawn, else the
  country's letters), the next battle, uptime, version and, under "Hosting
  Generously Donated By", the sponsor's logo (its name where there is no
  logo) and site (docs/server-browser.md). A server of another version is
  marked and can't be joined. The list renews itself every 30 seconds while
  it is up, never two renewals at once; one that fails keeps the list, says
  CAN'T UPDATE, RETRYING and tries again after 10 seconds. Moving between
  screens, the one left cascades out (its chosen item flashing last) as the
  next cascades in from the side it is reached from; bottom buttons both
  screens have stay put; the logo flies between the main menu's middle and
  the corner; the backdrop drifts in a step deeper per screen and parts for
  wide screens; and each move in plays the original's whoosh. The
  `menuTheme` setting picks the look (Battlefront (2004), Battlefront II
  (2005) or Eternal).

## Movies

The original plays Bink movies from `Movies/shell.mvs` behind every shell
screen; nothing in its backgrounds is live 3D. The port plays the same
movies straight from the player's game folder (`Movies/shell.mvs`, and an
add-on's `.mvs`), decoded by `rust/bfe-video` with the project's
[Bink decoder](https://github.com/centrevilletech/bink-decoder), without
their sound, each in a layer of its own under the interface. As the movie
config (`mcfg`) has it:

| Movies | Fade in | Fade out |
| --- | --- | --- |
| Intros (`main_trans`, `atat_in`, `coreship_in`...) | 0 | 0.2 s |
| The main menu's intro, `main_trans` | 0 | 0 |
| Screen loops (`start_screen`, `atat_screen`, `coreship_screen`...) | 0.1 s | 0.5 s |
| The main menu's loop, `main` | 0 | 0.5 s |
| The briefing's and Galactic Conquest's intros | 1 s | 1 s |
| The maps' flyovers (`<map>fly`) | 1 s | 1 s |

- A screen's movie is its intro, played once, then its loop for ever. The
  intro fades out over its last FadeOut seconds and the loop, loaded and
  waiting under it, starts as it ends, fading in over its FadeIn. The main
  menu's intro has no fades: it cuts straight into its loop.
- A new movie starts at once under the one showing, which fades out over
  its own FadeOut on top of it: from a loop, a half-second cross-fade, as
  the captures show going back from SINGLEPLAYER.
- Going back plays only the loop: no intro, no whoosh.
- Movies fill the screen cropped to keep their shape. The original
  stretches them to the screen.
- **Instant Action's preview** is the highlighted map's flyover in the
  lower right, in the rectangle the original works out for the screen
  (`preview_rect`: 530 x 400 at 800 x 600, so 678.4 x 512 at 1024 x 768),
  0.5 s after the highlight moves, behind the backdrop's hole. Only the
  AVAILABLE MAPS list drives it.
- A missing or broken movie (or no game folder at all, for data imported
  from another computer) leaves the still backdrop behind and lets the
  screen go on.

Each screen's movie and music (`SCREEN_MEDIA` in `rust/bfe-shell` `media`):

| Screen | Movie | Music |
| --- | --- | --- |
| The gate (the profile screen's place) | `start` | `shell_soundtrack` |
| MAIN MENU | `main` | `shell_soundtrack` |
| SINGLEPLAYER | `sp` | `shell_soundtrack` |
| OPTIONS | `options` | none (keeps what plays) |
| Instant Action, multiplayer | none (a still backdrop; Instant Action's flyover) | none |
| Briefing, Galactic Conquest | `briefing`, `meta`, `galaxy` | their own (disabled here) |

## Sound

The battle's engine (`rust/audio-control`) plays a battle's sound sub-level, a
bank of samples and streams. The front end's music and whooshes are plain
files in `shell.json`, so the front end has a small mixer of its own
(`rust/bfe-shell` `audio`, on the client's one audio stream), keeping the
original's buses and rules:

- **The buses.** The music on `shellmusic`, 0.6 of the music volume; the
  whooshes and menu sounds on `shellfx`, 0.3 of the interface volume (the
  gains of core.lvl's buses); the master volume over both. The volumes are
  the player's settings, live.
- **The music.** `shell_soundtrack` plays its tracks in turn, one at a
  time, with 10 s of silence between them (`SetShellMusicInterval`; the
  silence is measured against the interval as it is now, so the AUDIO
  OPTIONS page's 0 cuts one short, as in the original). A music name plays
  the tracks whose `properties` name it in `shell.json`, in their order:
  `shell_soundtrack` is five of the six, Galactic Conquest's is its own. A
  screen names the music it wants: a new name stops the old and starts it,
  the same name carries on, no name keeps what plays, `STOP` stops it. Each
  track plays through the `shellmusic` bus; it never goes straight to the
  speakers.
- **The moves between screens** (`ifs_movietrans`). Forward into a screen
  with an intro: if the screen's music isn't the one set (OPTIONS has
  none), `shellmusic` dips to 0 over a second; the intro starts with the
  next whoosh, `shell_transition1` to `10` in turn, which plays as the
  intro's first frame reaches the screen; when the intro ends,
  `shellmusic` comes back to 1 over a second, the screen's music is set
  and the screen shows. Between soundtrack screens
  (MAIN MENU and SINGLEPLAYER) there is no dip.
- **Menu sounds** (`shell_select_change`, `shell_menu_accept`,
  `shell_menu_exit`...) are asked for by name, as in a battle. They come
  from `shell.json`'s `sounds` when it has them; else from the front end's
  sound sub-level (`audio/shell/` in the prepared data), or failing that
  a battle's, whose `sound.json` has every core.lvl sound: each sample is
  read alone from its `bank.bin`, with the sound's own pitch, gain and
  PlayInterval.

## The loading screen

The original's loading screen is driven by Load/load.lvl's config and
each map's Load/<map>.lvl pictures. The port draws it from LAUNCH (or
JOIN) until the battle is ready and the sequence has ended.

### How the original runs it

The original's screen behaves like this:

- **When it moves.** The level loads on the game's one thread, and the
  loader updates the screen between the chunks it reads (and before each
  script it runs). An update less than 50 ms after the last does nothing,
  so the screen moves at most 20 times a second, each time by the real
  time since the last update, however long that was.
- **The timing** is the config's `TargetTime` pair, 3.5 and 1 in
  load.lvl (2 and 0.5 when a config leaves it out): each picture's lines
  track for the initial number of seconds, then the view zooms for the
  second. A picture takes 4.5 s; the three moves from the galaxy to the
  level take 13.5 s. Nothing in it waits for the load.
- **The lines.** Each picture's rectangle (`PlanetLevel`) is kept as
  fractions of the screen. At the start of a picture the crosshair is at
  the screen's edges. In the initial half of the tracking the horizontal
  pair moves, in the second half the vertical pair. At every update each
  moving line covers a share of what is left of its way, the share being
  how far into the half the update falls (from almost nothing to all of
  it), so the lines rush in and settle about 0.9 s into each 1.75 s half.
- **The pulse.** At every update of a half, if the leading line of the
  moving pair (the top one, then the left one) moved more than a
  thousandth of the screen, the pair's sound starts (`YTrackingSound`,
  load_ytracking, for the horizontal pair; `XTrackingSound`,
  load_xtracking, for the vertical); once it moves less, the sound stops.
  Both are looping sounds, so the pulse plays exactly while the lines can
  be seen moving and stops as they come to rest. One voice serves both,
  and it is started again if the sound system drops it.
- **The zoom.** `ZoomSound` (load_zoom) plays once at the zoom's first
  update. The crosshair opens from the rectangle to the screen's edges
  while the next picture grows out of it.
- **The transition.** As each zoom ends, the next picture takes over,
  `TransitionSound` (load_transition) plays once and the crosshair goes
  back to the edges. The last zoom, into the level, ends the sequence;
  the level picture is not held.
- **The end.** When the level has loaded, the game closes the screen, but
  first runs the sequence to its end at the same 20 updates a second: the
  screen always shows every picture. A sequence that ends first leaves the
  level picture and the disk up until the load is done.
- Without pictures there is no sequence: the screen closes with the load,
  and its one voice loops `BarSound` (load_bar, gain 0: silent).

### What the port draws

- **Four pictures**, each filling the screen (stretched, as the original):
  the galaxy, the planet in space (locale), its surface (landsat), the
  level under the blue hologram grid. On each of the initial three a pair of
  cyan lines tracks onto the next picture's rectangle on the original's
  timeline (`LoadingTimeline`): the horizontal pair in the initial 1.75 s,
  the vertical pair in the next, each line's approach the original's
  stepwise one made smooth (`approach()`: the same positions at every
  50 ms step, drawn every frame). Then the view zooms into the rectangle
  for 1 s (at an even rate of scale) while the next picture grows out of
  it, the lines riding its edges out to the screen's.
- **The pace** is the original's, on the clock alone: 13.5 s from the
  galaxy to the level, whatever the load does. The screen goes once both
  the sequence has ended and the battle is ready. Then it is gone at once,
  as the original cuts to the spawn screen. A third number in the timing
  (the port's) would hold the level that much longer.
- **The planet's name** (`planets.<planet>.name`) top left, 5 units into
  the safe area, in `gamefont_tiny`, over the initial three pictures.
- **The LOADING disk** bottom right, 252 units from the right and 223 up
  from the bottom of a 4:3 screen. On other screens it keeps to that
  corner (`diskPlace()`): its offsets and size scale with the screen's
  height (its width, below 4:3), never stretched, so a wide screen
  doesn't carry it in towards the middle. The disk is `load_rotating_disk`
  twice (240 and 176 units, turning both ways, so its thin ring falls
  outside and inside the lights), about a ring of 28 square lights whose
  lit arc chases round, the whole turned to 0.866 of its width; LOADING
  (`common.loading`) across it in `gamefont_medium`, tilted 14 degrees. Placements and sizes are measured
  on the captures. Without the disk's texture its segments are drawn.
- **The lights** (`lightLevel()`): the lit arc's head goes round the ring
  0.6 times a second. Behind it each light is a little darker than the one
  before, along a tail of three quarters of the ring that darkens fastest
  near the head, down to off; the light the head comes to rises over the
  28th of a turn before the head reaches it. How lit a light is depends
  smoothly on how far it is behind the head, so as the head moves each
  light fades in and out instead of jumping between shades. Its colour is
  mixed by that level between its look's colour stops (`mixColor()`).
- **The looks.** The disk and the planet's name follow the front end's
  look, the `menuTheme` setting. Battlefront (2004) is the original's:
  grey lights (#0b0d0f off, #5c6166 halfway, #b9bcbe at the head) on the
  texture's own cyan, in `gamefont_medium` and `gamefont_tiny`. Eternal has
  amber lights cooling to embers (#ffd27a at the head, the accent #ff9a1f,
  down to near black), the disk's segments as black glass with pale grey
  rings, and LOADING and the name in Saira, bold, off-white and spaced
  wide. Battlefront II (2005) has red lights (#ff6450 at the head, the
  accent #e0362a, down to dark navy) with a dark rim, navy segments with
  silver rings, and LOADING and the name in heavy white Saira outlined in
  black, as its buttons are. The texture is one colour with its shapes in
  its alpha, so each copy is recoloured: the segments in one colour, the
  thin ring's band in another. Saira is rasterized once when the content
  is admitted (`app/page_font.rs`); if it is missing the game's fonts stand
  in, in the look's colours.
- **The sound** (`rust/bfe-shell` `loading`, `audio`): the original's,
  from the same timeline. The timeline gives each pulse's start and end by
  the original's rule (from the 50 ms step in which the leading line moves
  more than a thousandth of the screen to the step in which it moves
  less), the zoom's one-shot as each zoom starts and the transition's as it
  ends. load_ytracking and load_xtracking loop one 0.35 s ping sample (a
  tone that dies away in 0.2 s) at pitch 1.8 and 2.1 and gain 0.7, so they
  pulse five or six times a second; load_zoom is 0.43 s at pitch 1.5.
  load_transition names a sample the install doesn't have and is skipped.
  All play on the `shellfx` bus (0.3 of the interface volume) under the
  master volume, from the front end's sound sub-level (`audio/shell`, else
  a battle's), planned against the audio clock so each is heard with the
  frame that shows its moment.
- **The load** runs beside the screen (`rust/bfe-client` `app/loading.rs`).
  A background thread reads the battle's content and admits the world (or,
  online, joins the server); the window thread keeps drawing the screen
  every frame and spends what is left of each 60 Hz frame on the GPU work
  (uploads, shaders), a step at a time, so the screen never stalls on it.
- **The hand-over.** Once the battle is ready and the sequence has ended,
  the battle is drawn under the screen for two frames, then the screen
  closes and the battle starts. Nothing moves in the battle before that,
  and nothing of it is drawn while the screen covers it. A network battle
  takes the same path. The sequence's length is fixed and the same on every
  computer: a fast load waits for it.
- Without the map's pictures it is the planet's name and the disk over
  `load_simple_backdrop` (the logo on the grid), or black, until the
  battle is ready; without fonts, only the disk. Jabba's Palace loads this
  way: it is an add-on (`AddOn/Tat3`), `load.lvl` has no `World` for it,
  and the add-on brings no pictures of its own. The name is its planet's
  (TATOOINE).

Not drawn yet: the rotating planet model, the side panels, the scan lines
and noise of the config (they don't show in the captures), and the
crosshair's corner pieces.

## Data read from shell.json

`rust/bfe-shell` reads the schema of [shell-assets.md](shell-assets.md)
and tolerates its variants:

- `movies`: keyed by the original's screen names (`ifs_start`, `ifs_main`,
  `ifs_sp`, `ifs_opt_contmain`, `ifs_sp_briefing`, `ifs_meta_top`,
  `ifs_meta_main`...) or the port's ids (`start`, `main`, `sp`, `options`,
  `briefing`, `meta`, `galaxy`); the reader gives both. `intro` and `loop`
  are movie names or `{ file, fadeIn, fadeOut }`. An entry's own
  `fadeIn`/`fadeOut` are its loop's, `introFadeIn`/`introFadeOut` its
  intro's; the movie config's fades fill in the rest.
- `maps[]`: `id`, `name` (or `nameKey`), `extracted`, `eras`, `fly`, and
  `load`: `{ planet (text or a planets.* key), stages: [{ image, target:
  [x, y, w, h] }] }`, the targets in the previous picture's pixels.
- `music[]`: `{ name, file, properties }`, `properties` the music names
  the track plays under (`shell_soundtrack` and its own), in the order the
  soundtrack cycles them; `whooshes[]`: paths, `shell_transition1` to `10`;
  `sounds`: `{ name: path or { file, gain, pitch } }` (optional).
- `load`: `{ backdrop, disk, crosshair: { horizontal, vertical, corner },
  text, targetTime: [3.5, 1] }`: the art as paths or as the names of
  `common.json` textures (4x where the raster pipeline made them); `text`
  is LOADING's string key. The timing is each picture's tracking and its
  zoom, in seconds; a third number, the port's, is how long the level
  holds after the last zoom (else not at all, as the original).
- `strings` join `common.json`'s; fonts are `common.json`'s with the 4x
  pages of `common/raster-hud/`.

## Checks

```sh
cd rust
cargo test --locked -p bfe-shell -p bfe-video
```

The tests cover the playlist (LAUNCH, the next battle through the playlist
and round again, the reshuffle each lap in random order, RESTART), the
music's cycle and gaps, screens' music, the moves (dip, whoosh, back), the
preview's rectangle, the gate's keys and pulse, reading `shell.json`, and
the loading screen's timeline against the original's stepwise lines and
pulse rule, the sequence's end, each sound against the moment of the
animation it goes with, the lights' gradient (each lit light darker than
the one before by a small step, none jumping from one frame to the next),
the looks' colours, and the look picked from the menu theme.

## Where the port differs

- The click gate is the port's own.
- The front end's own look is the port's: the original's menus put each
  screen over its own movie.
- Movies keep their shape on wide screens instead of stretching.
- The loading screen's disk and planet name take the front end's look
  (above); the Battlefront (2004) look's are the original's.
- The loading screen has no model, panels or noise yet (above); it draws
  every frame where the original drew at most 20 a second.
- Each map folder of an older import holds one mission: it plays for both
  eras, and the era picker offers only its era. A newer import holds every
  battle of the map, and each era plays its own.
