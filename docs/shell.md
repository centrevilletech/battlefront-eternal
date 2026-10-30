# The front end

The original's front end ("the shell"): the main menu, SINGLEPLAYER and
Instant Action with its playlist, OPTIONS, the multiplayer and profile
screens, over the original's background movies and music, and the
loading screen between it and a battle. The look is the original's; the
content is the port's (its settings, its maps).

It is built in two parts:

- **The media side and the plumbing** (this page): which page a URL is,
  the URLs between the front end and the battles, the playlist, the
  movies, the music, the click gate and the loading screen.
- **The screens** (`src/shell/screens.js`, on the menu framework of
  [menus.md](menus.md)): MAIN MENU, SINGLEPLAYER, Instant Action, OPTIONS,
  the multiplayer and profile screens, EXIT TO WINDOWS.

The original's behaviour follows its interface Lua (`ifs_main`,
`ifs_sp`, `ifs_missionselect`, `ifs_movietrans`, `ifelem_shellscreen`),
its movie config (shell.lvl `mcfg`) and Load/load.lvl's loading config,
and was checked against captures of the 2004 game. The data comes from
`tools/zero_shell.py` ([shell-assets.md](shell-assets.md)).

The profile is the player's name (the `playerName` setting), which a
server shows to the other players. Multiplayer needs one: joining a
server (from either browser, LAN, ONLINE or a direct connection) or
hosting one first asks for a name while the name is still the default
PLAYER 1, which DELETE PROFILE also returns to. Cancelling launches
nothing.

## Files

| File | What it does |
| --- | --- |
| `src/main.js` | The router: the battle (`src/game.js`) or the front end (`src/shell/`), each loaded only when it's the page |
| `src/launch.js` | Pure: which page a URL is, the battle URLs, the playlist in the URL, the next battle, the main menu's URL |
| `src/mission.js` | Pure: which of a map's battles a URL plays (`era`, `side`, `mission`), and that battle's mission, spawn screen and vehicles in map.json's places, for the page, the world worker and servers alike |
| `src/shell/index.js` | `createShell()`: the front end's page, from the gate to the main menu |
| `src/shell/screens.js` | Main menu, SINGLEPLAYER, Instant Action playlist, OPTIONS, profile and multiplayer screens |
| `src/shell/version.js` | The main menu's version line, bottom right: the desktop build's preview number (else WEB BUILD), the simulation (`bfe-sim-N`, which servers must match) and the game data's map, side and common generations (`M12 S2 C2`), with what the build needs when the data is stale (`bfeDesktop.about()`) |
| `src/shell/data.js` | `public/common/shell/shell.json` and `common.json`, read with fallbacks for whatever is missing |
| `src/shell/movies.js` | `MoviePlayer`: the background movies, their intros, loops and fades |
| `src/shell/audio.js` | `ShellMusic` (pure: the tracks in turn, the gaps) and `ShellAudio` (the buses, the dips, whooshes and menu sounds) |
| `src/shell/media.js` | Each screen's movie and music, and the moves between screens (`ifs_movietrans`); Instant Action's preview |
| `src/shell/backdrop.js` | The page theme's backdrop: a movie fading in down each side, under every screen; Instant Action's map preview in its frame |
| `src/shell/server-browser.js` | The server browser (the page theme's MULTIPLAYER): JOIN / CREATE / LEADERBOARD, the Internet's or the LAN's servers, filters, sorting, the pick, joining |
| `src/shell/server-browser-view.js` | The server browser on the page: the source and filters, the sortable list, GAME INFO (the map's flyover, the facts, the sponsor) |
| `src/shell/gate.js` | The click gate |
| `src/shell/text.js` | The game's bitmap fonts on a 2D canvas (the gate, the loading screen) |
| `src/shell/loading.js` | The loading screen: the original's timeline and its sounds (`LoadingTimeline`), the drawing, the hand-over, and `createLoadingScreen()` for the battle page |
| `src/shell/loading-worker.js` | The loading screen's own thread |
| `src/shell/loading-audio.js` | The loading screen's sound: the tracking pulse and the zooms, on the audio clock |
| `src/game.js` | `createGame({ onProgress })`: the load's steps for the loading screen, its GPU work a batch at a time; DONE's next battle |

## Pages and URLs

A page is a battle when its URL has any of `map`, `side`, `unit`, `spawn`
or `spawnscreen`; otherwise it is the front end. `?shell=1` forces the
front end, `?shell=0` the battle (on Theed, the battle's default map).
Every URL the tests and tools open names a battle, so they load as before;
`tests/launch.test.js` reads them all to make sure.

A battle is its URL, and the Instant Action playlist rides in it:

    /?map=geo1&era=cw&rotation=geo1:cw,hot1:gcw,rhn1:cw&round=0&order=random&noreload

| Key | What it is |
| --- | --- |
| `map` | This battle's map (the folder in `public/maps/`) |
| `era` | Its era, `cw` or `gcw`: the map's battle of that era (`src/mission.js`). A map without that era plays the one it has |
| `rotation` | The playlist, `map:era` entries, in the order they play this lap; 64 at most, as the original's mission queue |
| `round` | This battle's entry in it, from 0 |
| `order` | `random`: shuffle again at each wrap (RANDOM ORDER). Absent: PLAY IN ORDER |
| `shell`, `screen` | The front end's own: forcing it, and a screen to open on (`screen=main`) |

Every other key is a **dev flag** (`noreload`, `ai`, `tier`, `seed`,
`shadows`, `scale`, `audiodebug`...) and every URL built here keeps it.
`side`, `unit` and `spawn` choose the player's unit on one map, so they
don't carry over to another map; nor does `mission`, which names one
battle of a map by its mission script (`?map=nab2&mission=nab2a`; the only
way to play a historical `_h` version). `spawnscreen` is a dev flag that is
also a battle key: the main menu's URL keeps it and adds `shell=1`.

Which battle a URL plays is `src/mission.js`'s one rule, the same in the
page, the world worker and a server: `mission=`, else `era=`, else the
era of `side=` (`rep` and `cis` fight the Clone Wars, `all` and `imp` the
Galactic Civil War), else the map's Clone Wars battle, else its other.
Never a historical version unless named.

`src/launch.js`:

| Function | Gives |
| --- | --- |
| `isBattle(url)` | Whether the URL is a battle |
| `battleUrl({ map, era, rotation, round, order }, from)` | A battle's URL (entry `round` of `rotation`, or one map), with `from`'s dev flags |
| `launchUrl(entries, { random, rand }, from)` | LAUNCH: the playlist's first battle; shuffled first in random order |
| `nextUrl(from, { random })` | After a battle: `round + 1`; past the last, back to 0, shuffled again in random order; without a playlist, the same URL |
| `restartUrl(from)` | The same battle (RESTART MISSION) |
| `mainMenuUrl(from)`, `shellUrl(screen, from)` | The front end (`?screen=main`): the pause menu's QUIT |
| `readPlaylist(from)` | `{ map, era, entries, round, random }`, or null |
| `parseRotation`, `formatRotation`, `shuffle`, `MAX_PLAYLIST` | The playlist's text, the original's shuffle (7 x N swaps of two random entries), 64 |

`from` defaults to the page's own URL. LAUNCH (the Instant Action screen)
navigates with `location.assign()`, so the browser's Back returns
to the front end; the next battle replaces the page in the history
(`location.replace()`), so Back doesn't walk back through every battle
played.

## Instant Action

SINGLEPLAYER opens Instant Action, which builds the playlist: AVAILABLE MAPS
on one side, SELECTED MAPS on the other. Enter or a double click on an
available map adds it (an era picker opens when the map has two eras to
choose from) and on a selected one removes it; ADD and REMOVE do the same
with the pointer, and Delete removes the selected entry. Maps that are not
extracted stay in the list, dimmed, and cannot be added. The playlist holds
64 entries at most. PLAY IN ORDER / RANDOM ORDER sets `order`, and LAUNCH,
unavailable while the playlist is empty, opens the first battle with the
dev flags kept.

## After a battle

The original's DONE (or 30 s without input on the end-of-match table)
calls `ScriptCB_QuitFromStats`, which loads the next mission of the queue;
Instant Action's queue always loops, so it never runs out, and in RANDOM
ORDER it is shuffled again at every wrap. Here the scoreboard's DONE and
timeout call its port's `restart()`, which `src/game.js` makes
`location.replace(nextUrl())`: the playlist's next entry, or, for a battle
opened without a playlist, the same battle again (as before).

The only way back to the front end is the pause menu's QUIT
(`mainMenuUrl()`, [menus.md](menus.md)). The original goes back to the
MAIN MENU, not to the Instant Action screen, and so does the port.

## The front end's page

- **The click gate.** Browsers play no sound before a click or a key. The
  page opens on the original's first screen movie (`start_screen`, the
  profile screen's flythrough) with **PRESS ANY KEY OR CLICK TO BEGIN**
  (the port's own words: the PC game has no such line) pulsing at the
  bottom, as a focused button's label does (alpha 1 to 0.3 and back in
  0.7 s), in `gamefont_medium`. The press, any but the browser's own keys,
  starts the sound and the soundtrack, then moves on to the main menu with
  its intro and the first whoosh, as the original's profile OK does.
- **`?screen=main`** (the pause menu's QUIT) goes straight to the main menu
  with its intro. The sound starts then if the browser allows it after the
  battle page's clicks, else at the first click or key.
- **The page theme** (a design in progress) draws the front end's
  screens, popups and dialogs as a page over one backdrop, instead of the
  original's canvas over each screen's movie (`src/menus/themes/page`,
  docs/menus.md). One movie fades in down the left (the Clone Wars:
  `republic_win`, `republic_base`, `cis_win`) and one down the right (the
  Galactic Civil War: `imperial_win`, `rebel_base`, `rebel_win`), each
  crossing into the next, sharpened as the other backgrounds are
  (`backdrop.js`); they play on under every screen, so no screen plays a
  movie of its own. The gate shows the Battlefront Eternal logo (drawn in
  Saira, SIL OFL) over PRESS ANY KEY OR CLICK TO BEGIN; the main menu's
  rows come in under the logo; every other screen has its title at the
  top, a small logo in the corner and its BACK, middle and accept buttons
  along the bottom, their outer edges on the content's (a wide screen's
  panels, an option page's panel). Instant Action shows its lists either
  side of the highlighted map's flyover, in a frame of its own, the
  available maps under the game they're from (BATTLEFRONT (2004); the
  2005 game's are listed as planned). Option pages, Instant Action and the
  server browser darken the backdrop behind them. MULTIPLAYER is the
  server browser, all on one screen
  (`server-browser.js`): JOIN, CREATE (the hosting dialog) and LEADERBOARD
  (planned) tabs; INTERNET or LAN; a search, map, era and ping filters,
  hide full or empty; the servers under headers that sort them; and GAME
  INFO for the one picked: its map's flyover, mode, era, bots and teams,
  players, ping, place (a flag where the system draws one, else the
  country's letters), the next battle, uptime, version and, under
  "Hosting Generously Donated By", the sponsor's logo (its name where there
  is no logo) and site (docs/server-browser.md). A server of another
  version is marked and can't be joined. The list renews itself every 30
  seconds while it is up, never two renewals at once; one that fails keeps
  the list, says CAN'T UPDATE, RETRYING and tries again after 10 seconds.
  Moving between screens, the one
  left cascades out (its chosen item flashing last) as the next cascades in
  from the side it is reached from; bottom buttons both screens have stay
  put; the logo flies between the main menu's middle and the corner; the
  backdrop drifts in a step deeper per screen and parts for wide pages; and
  each move in plays the original's whoosh. The `menuTheme`
  setting picks the look; for now a dropdown at the top right sets it.
  `?menus=classic` brings back the original's menus on the canvas, each
  screen over its own movie, as the rest of this page describes them.
- `#hud` stays, empty, for load errors; `#stats` is hidden.
- `window.__shell` holds the data, the media and `enter()` for checks.

## Movies

The original plays Bink movies from `Movies/shell.mvs` behind every shell
screen; nothing in its backgrounds is live 3D. The port plays the same
movies transcoded to WebM, each in a muted `<video>` of its own under the
interface. As the movie config (`mcfg`) has it:

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
- Movies fill the screen cropped to keep their shape (`object-fit:
  cover`). The original stretches them to the screen; `fit: "fill"` does
  that.
- **Instant Action's preview** is the highlighted map's flyover in the
  lower right, in the rectangle the original works out for the screen
  (`previewRect()`: 530 x 400 at 800 x 600, so 678.4 x 512 at 1024 x 768),
  0.5 s after the highlight moves, behind the backdrop's hole. Only the
  AVAILABLE MAPS list drives it.
- A missing or broken movie leaves the hologram rooms' deep blue behind
  and lets the screen go on (Vite answers a missing file with its index
  page, which fails to decode).

Each screen's movie and music (`SCREEN_MEDIA` in `src/shell/media.js`):

| Screen | Movie | Music |
| --- | --- | --- |
| The gate (the profile screen's place) | `start` | `shell_soundtrack` |
| MAIN MENU | `main` | `shell_soundtrack` |
| SINGLEPLAYER | `sp` | `shell_soundtrack` |
| OPTIONS | `options` | none (keeps what plays) |
| Instant Action, multiplayer | none (a still backdrop; Instant Action's flyover) | none |
| Briefing, Galactic Conquest | `briefing`, `meta`, `galaxy` | their own (disabled here) |

## Sound

The battle's engine (`src/audio`) plays a battle's sound sub-level, a
bank of samples and streams. The front end's music and whooshes are plain
files in `shell.json`, so `ShellAudio` is a small player of its own on Web
Audio, keeping the original's buses and rules:

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
  track streams from an `<audio>` element into the `shellmusic` bus; it
  never plays straight to the speakers.
- **The moves between screens** (`ifs_movietrans`). Forward into a screen
  with an intro: if the screen's music isn't the one set (OPTIONS has
  none), `shellmusic` dips to 0 over a second; the intro starts with the
  next whoosh, `shell_transition1` to `10` in turn, which plays as the
  intro's first frame reaches the screen (a started video can take a
  moment to show it; measured, the whoosh is heard 24 to 35 ms after it);
  when the intro ends,
  `shellmusic` comes back to 1 over a second, the screen's music is set
  and the screen shows (`enter()` resolves). Between soundtrack screens
  (MAIN MENU and SINGLEPLAYER) there is no dip.
- **Menu sounds** (`shell_select_change`, `shell_menu_accept`,
  `shell_menu_exit`...) are asked for as in a battle,
  `emitSound("interface", name)`. They come from `shell.json`'s `sounds`
  when it has them; else from the front end's sound sub-level
  (`public/audio/shell/`), or failing that the first battle's
  (`public/audio/index.json`), whose `sound.json` has every core.lvl
  sound: each sample is fetched alone from its `bank.bin` by byte range,
  with the sound's own pitch, gain and PlayInterval.
- Nothing sounds before the click gate's press (the page's first click or
  key). A hidden tab goes quiet.

## The loading screen

The original's loading screen is driven by Load/load.lvl's config and
each map's Load/<map>.lvl pictures. The port draws it over the battle page
from the moment the page starts until the battle is ready.

### How the original runs it

The original's screen behaves like this:

- **When it moves.** The level loads on the game's one thread, and the
  loader updates the screen between the chunks it reads (and before each
  script it runs). An update less than 50 ms after the last does nothing,
  so the screen moves at most 20 times a second, each time by the real
  time since the last update, however long that was.
- **The timing** is the config's `TargetTime` pair, 3.5 and 1 in
  load.lvl (2 and 0.5 when a config leaves it out): each picture's lines
  track for the first number of seconds, then the view zooms for the
  second. A picture takes 4.5 s; the three moves from the galaxy to the
  level take 13.5 s. Nothing in it waits for the load.
- **The lines.** Each picture's rectangle (`PlanetLevel`) is kept as
  fractions of the screen. At the start of a picture the crosshair is at
  the screen's edges. In the first half of the tracking the horizontal
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
  level under the blue hologram grid. On each of the first three a pair of
  cyan lines tracks onto the next picture's rectangle on the original's
  timeline (`LoadingTimeline`): the horizontal pair in the first 1.75 s,
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
  the safe area, in `gamefont_tiny`, over the first three pictures.
- **The LOADING disk** bottom right, 252 units from the right and 223 up
  from the bottom: `load_rotating_disk` twice (240 and 176 units, turning
  both ways, so its thin ring falls outside and inside the lights), about a
  ring of 28 square lights whose lit arc chases round, the whole turned to
  0.866 of its width; LOADING (`common.loading`) across it in
  `gamefont_medium`, tilted 14 degrees. Placements and sizes are measured
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
  look, the `menuTheme` setting; with the original's menus
  (`?menus=classic`) they take the Battlefront (2004) look. The battle page
  is a fresh page, and the worker can't read the settings, so the page
  picks the look (`loadingLook()`) and hands it to the view.
  Battlefront (2004) is the original's: grey lights (#0b0d0f off, #5c6166
  halfway, #b9bcbe at the head) on the texture's own cyan, in
  `gamefont_medium` and `gamefont_tiny`. Eternal has amber lights cooling
  to embers (#ffd27a at the head, the accent #ff9a1f, down to near black),
  the disk's segments as black glass with pale grey rings, and LOADING and
  the name in the page's Saira, bold, off-white and spaced wide.
  Battlefront II (2005) has red lights (#ff6450 at the head, the accent
  #e0362a, down to dark navy) with a dark rim, navy segments with silver
  rings, and LOADING and the name in heavy white Saira outlined in black,
  as its buttons are. The texture is one colour with its shapes in its
  alpha, so each copy is recoloured on a canvas: the segments in one
  colour, the thin ring's band in another. Saira is loaded with the
  `FontFace` API into the fonts of the thread that draws (a worker has its
  own); no text is drawn until it is in, and if it fails the game's fonts
  stand in, in the look's colours.
- **The sound** (`src/shell/loading-audio.js`): the original's, from the
  same timeline. `LoadingTimeline.sounds()` gives each pulse's start and
  end by the original's rule (`pulse()`: from the 50 ms step in which the
  leading line moves more than a thousandth of the screen to the first in
  which it moves less), the zoom's one-shot as each zoom starts and the
  transition's as it ends. load_ytracking and load_xtracking loop one
  0.35 s ping sample (a tone that dies away in 0.2 s) at pitch 1.8 and
  2.1 and gain 0.7, so they pulse five or six times a second; load_zoom is
  0.43 s at pitch 1.5. load_transition names a sample the install doesn't
  have and is skipped. All play on the `shellfx` bus (0.3 of the interface
  volume) under the master volume, from the front end's sound sub-level
  (`public/audio/shell`, else a battle's), each sample fetched by byte
  range. The battle page gets an `AudioContext` of its own as the screen
  opens; browsers that remember a click on the site (the gate's,
  LAUNCH's; the desktop app needs none) let it run at once, otherwise
  what is left of the sequence plays from the page's first click or key.
  It closes with the screen. `?sound=0` leaves the sound out.
- **Keeping the sound with the pictures.** The screen draws in a worker
  and the sound plays on the page, whose thread the battle's load holds
  for up to a couple of seconds at a time; sounds started as messages
  arrived came late by as much (measured: a zoom sound 1.5 s after its
  zoom with the page held for 2 s). Now nothing about the sound waits on
  the page once the sequence starts. The worker loads the pictures,
  uploads them to the GPU (each drawn once, a pixel big), and sends the
  page the sequence's plan. The page picks the start 0.1 s ahead and puts
  every sound of the sequence on the `AudioContext`'s own clock at once
  (its output timestamp ties that clock to the page's), each to be heard
  1/30 s after its moment, about when the first frame drawn at that
  moment reaches the screen; then it tells the worker the start, and the
  worker draws every frame from `performance.timeOrigin + now` against
  it. A busy page can only delay the start, sound and pictures together.
- **The load's progress** comes from `createGame({ onProgress })`:
  `onProgress("data")` once the map's files are in (0.3),
  `onProgress("world")` once the world is built (0.5),
  `onProgress("textures", loaded, total)` as textures arrive (0.5 to 0.8),
  `onProgress("shaders")` after the shader warm-up (0.85); the game's
  `ready` is 1. It no longer paces the pictures (the original's doesn't);
  it is kept for checks.
- **The hand-over** (`handover()`): once the battle is loaded
  (`game.loaded()`: every texture on the GPU, the shaders compiled; or
  60 s) and the sequence has ended (or 30 s more, for pictures that never
  come), the battle is drawn under the screen for two frames
  (`game.unveil()`), then the screen closes and `game.begin()` starts the
  battle. Nothing moves in the battle before that, and nothing of it is
  drawn while the screen covers it. A network battle (`?network=1`) takes
  the same path: `src/main.js` is the same for both, `createGame` returns
  once the server's welcome is in, and the client tells the server it is
  suspended until `begin()`.
- **Its own thread, and the GPU's.** The battle's load holds the page's
  thread for one to two seconds at a time (building the world, compiling
  shaders), so the screen draws in a worker on an `OffscreenCanvas`, on
  every animation frame; where the browser has none, it draws on the
  page. A blocked page thread leaves the worker's frames alone. What held
  them up, measured in Chromium's traces on an integrated GPU (ANGLE on
  OpenGL), is the GPU process's main thread, which runs the battle's
  WebGL commands, the screen's canvas and the compositor that puts both
  on screen in turn. The battle's shader warm-up gave it some fifty
  programs at once (ANGLE translates each shader there, 1 to 6 ms apiece,
  then links it), and `game.loaded()`'s uploads, 6 ms of the page's time
  per frame, queued 50 to 80 ms tasks back to back: gaps of 170 to 390 ms
  in the screen once or twice a load. Now `graphics.warmupPaced()`
  compiles a program at a time and `game.loaded()` uploads 1 MB at a
  time, each batch waited for with a fence (`gpuIdle()` in
  `src/game.js`), and the battle draws nothing under the screen until its
  last two frames. `window.__loading.frames` counts the screen's frames,
  its worst gap and its gaps over 50 ms, with when they fell.
- `?loading=0` leaves it out. `window.__loading` has its progress, the
  sequence's `plan`, `start` and `endsAt`, the view's last state
  (`sequence`: stage, phase, `done`, `cue`, and `shown`, the first frame
  drawn at each sound's moment), `frames` and `sound.state()` (the sounds
  put on the clock and when each is heard) for checks. The canvas takes
  no pointer events, so nothing under it is blocked.
- Without the map's pictures it is the planet's name and the disk over
  `load_simple_backdrop` (the logo on the grid), or black, until the
  battle is ready; without fonts, only the disk. Jabba's Palace loads this
  way: it is an add-on (`AddOn/Tat3`), `load.lvl` has no `World` for it,
  and the add-on brings no pictures of its own. The name is its planet's
  (TATOOINE).

Not drawn yet: the rotating planet model, the side panels, the scan lines
and noise of the config (they don't show in the captures), and the
crosshair's corner pieces.

Measured on Geonosis (`?map=geo1&ai=off&shadows=off`) in headless
Chromium on an integrated GPU, before and after the loading screen's second
pass:

| | Before | After |
| --- | --- | --- |
| Screen gaps over 50 ms per load | 1 to 3, worst 169 to 391 ms | 0 in most loads (worst 17 to 50 ms); at most one of 67 to 100 ms on a cold shader cache with the machine busy |
| Sounds against their frames | each started when its message reached the page: 0 to 21 ms late with the page free, 250 ms and 1.5 s late with it busy | each heard 1/30 s after its moment (0.0 ms off the plan), 18 to 33 ms after the first frame showing it, the page held for 2 s or not |
| Pulse length | the whole phase (0.5 s and 2 s), on past still lines | while the lines move (0.9 and 0.85 s) |
| Battle ready | 3.6 to 4.3 s | 4.1 to 6.7 s (the paced warm-up and uploads), inside the 13.5 s sequence |

Under SwiftShader every shader link and upload is software, and the
screen's own frames are slow; the checks only report its gaps there.

## Data read from shell.json

`src/shell/data.js` reads the schema of [shell-assets.md](shell-assets.md)
and tolerates its variants:

- `movies`: keyed by the original's screen names (`ifs_start`, `ifs_main`,
  `ifs_sp`, `ifs_opt_contmain`, `ifs_sp_briefing`, `ifs_meta_top`,
  `ifs_meta_main`...) or the port's ids (`start`, `main`, `sp`, `options`,
  `briefing`, `meta`, `galaxy`); the reader gives both. `intro` and `loop`
  are paths (relative to `public/common/shell/`) or `{ file, fadeIn,
  fadeOut }`. An entry's own
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
  pages of `public/common/raster-hud/`.

## Checks

- `node --test tests/launch.test.js`: the router (every test URL is still a
  battle), the URLs, LAUNCH, the next battle through the playlist and
  round again, the reshuffle each lap in random order, the main menu's URL.
- `tests/shell-media.test.js`: the music's cycle and gaps, screens' music,
  the moves (dip, whoosh, back), the preview's rectangle, the gate's keys
  and pulse. `tests/shell-data.test.js`: reading `shell.json`.
  `tests/shell-loading.test.js`: the loading screen's timeline against the
  original's stepwise lines and pulse rule, the sequence's end, each sound
  against the moment of the animation it goes with (within a
  millisecond), the whole plan on a stand-in `AudioContext`'s clock (and
  joined late), the hand-over's waits and caps, the lights' gradient (each
  lit light darker than the one before by a small step, each its own
  shade, none jumping from one frame to the next), the looks' colours, and
  the look picked from the menu theme.
- `PLAYWRIGHT_MODULE=... node tests/shell-browser.mjs`: the front end,
  the gate, the press, the music's tracks, a dip, a menu sound, Back,
  `?screen=main`, Instant Action's preview, a missing `shell.json`, and
  battles booting under the loading screen (its progress following the
  load's steps), with screenshots in the folder `SHELL_SHOTS` names. It serves
  synthetic data it makes with ffmpeg and ImageMagick (test patterns,
  tones, drawn pictures, and a tiny sound sub-level of pings for the menu
  sound and the loading screen's), so it can name what plays; `SHELL_CHECKS` picks
  checks by name, `SHELL_TEST_URL` another server, `CHROMIUM` a browser
  of the machine's instead of Playwright's download.
- `PLAYWRIGHT_MODULE=... CHROMIUM=/usr/bin/chromium node
  tests/shell-loading-browser.mjs`: the loading screen over a real load
  of the server's data (Geonosis; `LOADING_QUERY` picks another): every
  picture loads and goes by, the sequence lasts the original's 13.5 s, the
  screen goes only after both the sequence's end and the battle's
  `ready`, the three sounds load, a pulse for each pair of lines and a
  zoom sound for each zoom go on the audio clock, each heard as planned
  and after the frame that shows its moment, `?loading=0` leaves it all
  out; it prints the load's steps, the timeline, the sounds against their
  frames, the screen's frame gaps and the page's long tasks. With
  `LOADING_GPU=gl` (or `vulkan`) Chromium draws on the machine's GPU and
  the screen must draw without gaps over 120 ms.

## Where the port differs

- The click gate is the port's: browsers need a click before sound.
- The page theme (above) is the port's own: the original's menus, on the
  canvas over each screen's movie, are `?menus=classic`.
- Movies keep their shape on wide screens instead of stretching.
- The loading screen's disk and planet name take the front end's look
  (above); the Battlefront (2004) look's are the original's.
- The loading screen has no model, panels or noise yet (above); it draws
  every frame where the original drew at most 20 a second.
- The era of a playlist entry rides in the URL, but each map folder of an
  older import holds one mission: it plays for both eras, and the era picker
  offers only its era. A newer import holds every battle of the map, and each
  era plays its own (`src/mission.js`).
