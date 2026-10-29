# The spawn screen

The original's spawn screen: unit and class select, team switch and spawn
post, before every life. It has two sides here:

- **The screen** (this document, `src/spawn-screen/`): what is drawn and how
  input makes a choice. It reads the world only through the spawn port, and
  its choice goes back as a world command.
- **The world side** ([human-spawn.md](human-spawn.md)): when the player may
  spawn, what he spawns as and where, his team, and the bodies he plays.

The original's behaviour follows its interface Lua and was measured on
screenshots of the 2004 game: Geonosis (Republic), Kamino (CIS) and Hoth
(Empire). The assets come from [spawn-screen-assets.md](spawn-screen-assets.md).

## Files

| File | What it does |
| --- | --- |
| `src/spawn-screen/index.js` | `createSpawnScreen()`: opens and closes the screen, drives the model, input and view, plays the sounds, sets the camera shot, frees the mouse. `pauseRespawn()`: the pause menu's RESPAWN. |
| `src/spawn-screen/model.js` | Pure: the screen's state (closed, choosing, committed) and the choice (team, class, post), with the original's rules for each |
| `src/spawn-screen/input.js` | Keys and mouse to model actions, once per frame; `bindDom()` feeds it from the page |
| `src/spawn-screen/classic/view.js` | The Classic screen, as a layer of the battle HUD |
| `src/spawn-screen/classic/layout.js` | Pure: where each piece sits for an interface height |
| `src/spawn-screen/page/view.js` | The page view: the screen in the front end's look (see [The page view](#the-page-view)) |
| `src/spawn-screen/page/stage.js` | The page view's 3D stage on the HUD's canvas: the body on its projector, the room or the vignette, the hologram map |
| `src/spawn-screen/page/rig.js` | The stage's camera: the framing (a lens shift), the sweep in, the swings, the drift |
| `src/spawn-screen/page/holomap.js` | The battlefield projected as a hologram: the HUD's spawn map drawn into a texture, the disc, the pins, the projector, picking |
| `src/spawn-screen/page/rooms.js` | The 2004 look's hologram room (the `shell_cube` texture), and the floor of light over the battle |
| `src/spawn-screen/page/director.js` | The battle behind the page view: the mission's camera shots, drifting, cut to the one that sees the post picked |
| `src/interface/flashy.js` | The window titles' intro animation (the shared interface kit, docs/menus.md) |
| `src/interface/models.js` | The interface's 3D: its material and lights, model scales, its camera |
| `src/interface/shapes.js` | The windows' 9-slice, text wrapping, the hotspots' point-in-quad test |
| `src/spawn-screen/classic/preview.js` | The picked class's body, standing and lit |
| `src/spawn-screen/debug.js` | The screen on demand over the running game |
| `src/spawn-screen/policy.js` | The world side's: whether the screen runs (see human-spawn.md) |
| `src/hud.js` | The layer hook, `drawBattlefield()` and the spawn map; `toInterface()` from the interface kit (`src/interface/`) |
| `src/pause/index.js` | RESPAWN in the pause menu (`enableRespawn()`) |
| `src/audio/game.js` | The `interface` sound event |

## When it shows

- `src/game.js` builds the screen only in the page, and only when the
  policy says it runs: no `side=`, `unit=` or `spawn=`, or `spawnscreen=1`.
  With them the world confirms the URL's choice by itself, as before.
- It opens when the map loads (the player starts `unspawned`) and on the
  port's `awaitingSpawn`, four seconds after a death. Nothing shows during
  those four seconds.
- It closes on `spawned`, when the player gets a unit back any other way,
  and when the battle is over (`humanState().over`); after that it doesn't
  open again.
- The world keeps running behind it. The pause menu over it (Esc) pauses
  both, as the original's does, and shows only the screen's room and the
  class's body under GAME PAUSED ([menus.md](menus.md)).

## The choice

As the original's `SpawnDisplay` makes it:

- **Opening:** the team and class are the player's (class 0 at the start,
  the last class played after a death). The post is the one remembered from
  the last life, checked again: kept while eligible, else the eligible post
  nearest it, else the front line (`port.choosePost`), else a random
  eligible post.
- **Class:** Up wraps from the first class to the last; Down is
  (index + 1) mod count. Heroes aren't offered.
- **Team:** Left or Right, or a click on the logos, switches between teams 1
  and 2 if the mission allows it. The class goes back to 0, and the post is
  a random eligible post of the new team, checked as above. Nothing reaches
  the world until SPAWN.
- **Post:** Tab takes the team's next eligible post by a counter of the
  team's own. Held, it takes another every 0.2 s. A click takes only a post
  the picked team itself holds; allied posts take Tab or the default pick.
  The post is checked every frame, so a lost post moves the pick to the
  nearest eligible one.
- **SPAWN:** a click on SPAWN (a ±32 unit square at its middle), the right
  mouse button, or Space held. Space counts while held, so holding it as the
  screen opens spawns at once. The choice goes to the world as
  `port.request({ team, className, post })`. A request the world hasn't
  acted on within a second gives the choice back and withdraws it
  (`port.cancel()`, the original's un-commit), as does a close for any
  reason but his spawning (the battle's end).

## Input

`SpawnInput` keeps the keys held and queues presses. Once per frame it
applies them in order:

| Action | Keys and mouse |
| --- | --- |
| Previous / next class | Up / Down, wheel up / down |
| Switch team | Left or Right, a click on the logos |
| Pick a class | A click on a class window, its title bar included |
| Pick a post | Tab (held: every 0.2 s), a click on one of the team's own posts |
| Spawn | Space held, the right mouse button, a click on SPAWN |
| Click at the pointer | Enter, numpad Enter |
| Pause menu | Esc |

- The frame after any frame that pressed a class or team key ignores those
  keys, as the original's does.
- A click is tested against each hotspot as it shows on screen: the
  rectangle in the element's own turned plane, projected.
- The pointer starts in the middle of the screen when it opens, so Enter on
  its own does nothing.
- There is no gamepad input in the port, so the original's pad bindings
  aren't here.

**The mouse.** While the screen is open the game's controls are off
(`controls.enabled`), the pointer lock is released, and the page's cursor
is hidden over the canvas, where the screen draws `mousecursor` instead.
SPAWN asks for the pointer lock again from inside the click or key that
pressed it, while the browser still allows it. Esc opens the pause menu
over the screen; closing it, even with RESUME, comes back to the screen
with the mouse still free. The screen frees the mouse with
`controls.release()`, the game's own release, so it doesn't pause the
game.

## Drawing

The Classic view is a layer of the battle HUD (`hud.setLayer()`). While it
is set, the HUD hides the player's own displays (weapons, health, reticle,
capture, damage, stats, vehicle) and the message feed. The reinforcements
count from the picked team and move 10 units left. The map shows the
battlefield in the spawn layout.

It all sits in the HUD's interface space: 1024 units across and
H = 768 + (h/w − 0.75) × 768 down, drawn through the HUD's 60 × 46.8 degree
camera onto the plane 886.81 units out. So turned windows really are in
perspective. The layout is the Lua's, inside the safe area (90% of the
screen, from 5% in). At 4:3:

| Piece | Where | Size | Turned |
| --- | --- | --- | --- |
| Five class windows | x 212.5; y 124.8 + i × (0.13 sh + 18 + 10) | 322.6 × 89.9 | 35° about their vertical axis, the right edge away |
| Objectives window | (834.6, 124.8) | 276.5 × 103.7 | −20° |
| Spawn point window | (834.6, 677.8) | 276.5 × 103.7 | −20° |
| SPAWN | (512, 696.7) | 184.3 × 82.9 | 45° about its horizontal axis, the top edge away; the word lies flat |
| Team logos | the picked team's at (465.9, 128.3), the other at (558.1, 128.3) | | spinning |
| The class's body | feet at (W/2, H/1.2) | about 385 units tall | facing the viewer |
| Map | (809, H/2) | 419.8 across | |

The 18 in the window spacing is gamefont_tiny's line height, the title
bar's height.

**Windows.** `border_3_pieces`, nine-sliced with 16-unit corners. Each
class window has the class's name on an orange title bar (`headerbuttonleft`,
`-mid`, `-right`, the end pieces square) and its weapons in gamefont_tiny at
0.13 H / 135. The picked class's title and weapons show at full strength,
the others at half; the windows themselves don't change. The objectives
window lists the team's objectives, wrapped and left aligned; the spawn
point window has the post's name and the vehicles it gives the team, or
"NONE".

**Titles.** Each title plays an intro: the bar grows left to right (0.1 s),
a white block (`cursor`) fades in at the text (0.05 s) and sweeps across it
(0.05 s), the text grows from its centre line (0.15 s), then blinks out and
fades back in (0.15 s). The class titles replay on opening and on a team
switch, 0.05 s apart from the top; the post's title replays when the post
changes. Kamino's retail screenshot, taken half a second after the screen
opened, shows the five titles at those stages.

**SPAWN.** `opaque_rect`, nine-sliced, tipped back 45 degrees in true
perspective. Seen from that far below the screen's middle the tip barely
shows: the box is about 79 units tall, 177 across at the top and 189 at
the bottom, as the screenshots measure. "SPAWN" itself isn't tipped.

**The team logos.** The command post holograms (`com_icon_*`) as 3D models,
lit by the interface's light, the picked team's green (32, 223, 32) and the
other red (223, 32, 32) at 35% strength. A switch swaps them over 0.25 s:
each slides to its new place, growing or shrinking between the size
parameters 0.5 and 0.25 × sw/800, spinning at −600°/s; after that they spin
at −9°/s. Before the first swap they turn at +6°/s. When the mission
forbids switching, the logos and their hotspot are gone. The hotspot is this
port's own size: the original's 38.4-unit square between the logos reaches
only their inner halves, so a click anywhere on either logo, with a margin
(0.05 sw + 60 across each way, 55 up and down), switches team.

**The reinforcements.** The HUD's own counter, as the picked team sees it,
10 units left. A team switch by mouse turns it over: π back to 0 about its
vertical axis at 15 rad/s.

**The map.** The HUD's full battlefield (`drawBattlefield()`), in the spawn
layout:

- 328 × W/800 across, 128 × W/800 left of the safe area's right middle;
- the map through `map_mask`;
- posts, vehicles and the radar's icons as the picked team sees them
  (relation colours; enemies only where a friend sees them);
- a post being taken blinks toward the taker's colour for the first quarter
  of each second;
- a command vehicle (Hoth's AT-ATs, Geonosis' AT-TEs) is a post while it
  lives ([human-spawn.md](human-spawn.md), "Command vehicles"): it shows as
  its vehicle icon, where it is now, and Tab and a click pick it as they
  pick a post; the spawn point window names it by its label (AT-AT);
- the picked post gets a green ring, lit every frame while picked and fading
  at 10 per second after. Its size is the icon's plus 3 × that highlight,
  its line at 0.86 of that: 33 units across when lit, as measured;
- no post names.

The world-to-map mapping is the minimap's (the posts' box plus 100 m). It
puts Kamino's and Hoth's posts on the retail screenshots' to the pixel.

**The room.** `shell_cube`: two walls meeting in a corner and a floor, its
grid texture scrolling 0.0625 a second. The original places an interface
model at its screen point's place on the plane times a factor, then scales
it. For the room that factor is 0.009 and the scale 1.6, at the screen's
middle. So the eye sits 7.98 / 1.6 = 5.0 room units in front of the model's
own origin, and the corner falls left of centre. Fitting the grid lines of
all three retail screenshots gives that same 5.0 independently. The room
isn't lit. Its vertices carry tints the common model pack drops: grey 192,
and 68 at the open end of the right-hand wall, which is why the
screenshots' right edge is darker. The screen puts them back
(`roomTints()`). It is drawn under everything else.

**The class's body.** The port's `previewBody(className)`: the class
holding its primary weapon in the idle stance, fidgeting every 3 to 8 s; a
droideka cycles its own states. The view swaps its materials for the
interface's, on the same textures. The light is ambient (144, 144, 144)
plus a white light from the front left above, clamped at full. Scale:
0.936 / (0.006 / (W/800)) = 200 units a metre, 0.9 of that for a droideka,
which gives the clone trooper's 385 units.

**The pointer.** `mousecursor`, 32 units, drawn last.

Every texture comes through `hud.texture()`, so the 4× WebP atlases apply
(`rasterHud` in common.json), as do the upscaled font pages. A missing
texture falls back to flat colour.

**Other screens.** The 3D models use a camera of the window's own shape
with the HUD's vertical field of view. So at 16:9 they keep their
proportions and their share of the screen's height, while the flat pieces
follow the HUD's interface space as everything else in the HUD does.

## Sounds

Through the audio events (`emitSound("interface", effect)`), by the
mission's `SetSoundEffect` names:

- `SpawnDisplaySpawnPointChange` (`shell_select_change`) on any class, team
  or post change, the post's default pick on opening included. Its 0.05 s
  play interval merges a team switch's several into one.
- `SpawnDisplaySpawnPointAccept` (`shell_menu_enter`) when the screen closes,
  which in practice is when he spawns.

The RESPAWN question's own sounds (`shell_menu_accept`, `shell_menu_cancel`)
aren't in the extracted banks, so it is silent.

## The camera

On every opening but the battle's first, the screen asks the port to show
the world from one of the mission's camera shots, picked at random. On the
first, where the original cuts nowhere, it shows the first shot: Kamino's
load.png, taken then, has shot 0's lit pillars and round platform behind
the room, which shots 1 and 2 don't. Its pillars stand a little further
apart than the port's shot 0 puts them, so the original may frame shots
with a narrower view. On closing the screen hands the camera back
(`setViewShot(null)`). The world's own camera never moves.

The page view moves through the shots itself (`directsShots`; see [The
page view](#the-page-view)): each shot is held for 11 seconds, panning
4.5 degrees and pushing 2.5 m along its own view, then cuts to the next. A
post picked brings up the shot that best sees it (near it, looking its
way), turned 40% of the way toward it. Only turns and a short push, so the
camera stays where the mission's author put it. A shot that moves on from
the last (`setViewShot(shot, { cut: false })`) keeps the temporal
upscaler's history; a cut drops it.

## The pause menu's RESPAWN

The pause menu (src/pause) lists RESPAWN (`game.pause.suicide`) always, as
the original's does, with or without the screen. It asks for confirmation
(`game.pause.suicide_prompt`), and YES calls `port.suicide()` if the soldier
is alive: a normal death, a ticket spent, then the screen four seconds
later.

## The page view

`src/spawn-screen/page/` draws the same screen, model and input in the
front end's look (src/menus/themes/page, docs/menus.md): the battle
passes `view: "page"` unless the URL has `menus=classic`, which keeps the
Classic view. The layout is the original's: the classes down the left,
the soldier in the middle with SPAWN under him, the objectives and the
map down the right, the teams across the top. The middle and the map are
a 3D stage on the HUD's canvas:

- **The stage** (`stage.js`), a scene in metres with the soldier's feet at
  the origin: the body (the Classic view's `ClassPreview`) on a hologram
  projector (a ring of light turning on the floor, a glow, a column of
  light with scan lines, motes rising), lit in the look's light (the
  interface material's optional key tint and rim, `src/interface/
  models.js`). A new class builds up from its feet behind a line of light
  (the material's optional `cut`).
- **The camera** (`rig.js`) frames the soldier into the page's place for
  him with a lens shift, so he keeps his place while it moves. Opening, it
  sweeps in from his right, high and far back; another class swings it a
  little, another team further; it turns a little with the pointer and
  drifts; SPAWN pushes it in as the page flashes. A drag on him turns him
  (flung, he slows, then turns back to face the camera after two seconds);
  dragging up or down tilts the camera.
- **The map** (`holomap.js`) is projected as a hologram over a projector:
  the HUD's own spawn map (`drawBattlefield()`: the posts, their names
  and blinking, vehicles, units, the picked post's ring) drawn into a
  texture every frame, on a disc tilted back 44 degrees, its picture in the
  hologram's colour (the posts keep theirs), with scan lines, a rolling
  band and a turning rim; each post a pin of light, taller where picked
  or under the pointer, a ripple running out from one picked; a cone of
  light up from the projector; another team's map projects again. The HUD
  doesn't draw its own spawn map while the screen is open. A press on the
  disc picks the team's post nearest it (a ray from the camera), or the
  post whose pin it meets.
- **By look:** the 2004 look stands it all in its hologram room (the
  `shell_cube` panels on the walls, floor and ceiling of a room round the
  camera, scrolling, light running along their lines; `rooms.js`), the
  battle hidden. The others show the battle behind through the mission's
  camera shots (see [The camera](#the-camera)), darkened to a vignette
  with a pool of light round the soldier, and a floor of light under him
  with rings rippling out. The 2005 look's projector is a cone, the
  others' a low emitter. The colours are the look's custom properties
  (`--bfe-spawn-*`).
- **The teams** across the top, as a choice of two in team order: each
  with its logo (the original's `com_icon` models) turning, its name, its
  reinforcements and a bar of them. The joined team is lit in green, YOUR
  TEAM; the other's logo is red and smaller, and it carries SWITCH TEAM.
  A click on it or on the swap between them, or Left or Right (as the
  original's keys), switches; its logo wakes under the pointer. Where the
  server assigns teams it says TEAMS: AUTO ASSIGN (the original's string)
  and takes no click. A countdown under them for a team holding no
  posts.
- **SELECT SOLDIER** down the left: a window a class, its name on a bar
  (the 2004 look's orange, the 2005 look's steel capsules), its weapons
  under it, the picked one lit; when they don't all fit, the others fold
  to a line. Under the soldier, **the class's name**, the post picked and
  its vehicles, and **SPAWN** (breathing while it can go).
- The pause menu over it hides it (`hidePointer`); the page's own pointer
  is used (`systemPointer`). Presses on its elements go through the
  screen's input as the Classic view's hotspots do (`hit()` reads the
  element under an interface point, the hologram's posts by a ray), so
  keys, the wheel and the right button work as before.

## Not the original's

- The original's gamepad bindings, the multiplayer wait (`ifs_readyselect`,
  "LAUNCH IN:") and its BACK, and the tooltips (off unless the profile turns
  them on) aren't here.
- The original warps the mouse to the screen's middle. A page can't, so the
  drawn pointer starts there and follows the mouse once it moves.
- Measured from the screenshots rather than taken from the Lua: the title
  bar's extent (from 1.75 units in at the left to 3.9 short of the right, and
  from a line plus 6.3 units above the window to 0.3 short of it; the Lua's
  text box is smaller); the weapon and objective lists centred as one line
  longer than they are, up to what their box holds (the original ends them in
  a line break); SPAWN at its texture's own alpha (the Lua gives it 128, which
  the screenshots don't show); the light's direction.
- The room's vertex tints are restored by a rule on the model's own
  vertices (x > 0.4 and z > 1 are the dark ones), since the hologram pack
  has no vertex colours. Exporting them would make that rule unnecessary.
- No `hud_map_raster` scan lines over the map: none of the three retail
  screenshots shows any (Kamino's lines are its map texture's own, 10.6
  units apart), so they aren't drawn.

## Checks

```sh
node --test tests/spawn-screen-model.test.js tests/spawn-screen-input.test.js tests/spawn-screen-index.test.js
PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/spawn-screen-browser.mjs
```

- The node tests cover the model's rules, the input's bindings and quirks,
  and the screen's opening, closing, sounds, camera and withdrawn choices,
  against a fake port (`tests/spawn-screen-fake-port.js`).
- The browser test loads geo1, kam1 and hot1 with the screen, renders each
  at 1024 × 768 and 1280 × 720 into the folder `SPAWN_SHOTS` names, and on
  geo1 plays it: a team switch and back, a class picked by a click, SPAWN,
  RESPAWN from the pause menu, the screen back after the death, the pause
  menu over it. A run with `unit=` checks the screen is never built.
- To show the screen over a living player, from the console:

  ```js
  const { livePort } = await import("/tests/spawn-screen-fake-port.js");
  const { spawnScreenDebug } = await import("/src/spawn-screen/debug.js");
  await spawnScreenDebug(__bf, await livePort(__bf));
  ```
