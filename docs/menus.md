# The menus

The original's menus, as one framework: the widgets and screens the 2004
interface builds every menu from, drawn in its interface space, and the
pause menu built on them. The front end ([shell.md](shell.md)) builds its
screens on the same pieces. The look is the original's; what the option
pages hold is this port's own settings.

The original's behaviour follows its interface Lua (`interface_util`,
`ifelem_*`, `ifs_pausemenu`, `ifs_opt_*`, `popup_yesno`) and its "flashy"
text intro, and was measured on captures of the 2004 game at 1024 × 768 (the
pause menu, its OPTIONS and their four pages, the YES / NO popups).

## Files

| File | What it does |
| --- | --- |
| `src/interface/surface.js` | `InterfaceSurface`: the renderer, textures by name (4x atlases where made), fonts, strings, a quad batch, the interface camera; `drawFrame()` for a page that is only an interface; `commonTextureUrl()` |
| `src/interface/space.js` | The interface space: 1024 × H units, the camera, `anchorMatrix()`, `planePoint()` |
| `src/interface/batch.js`, `fonts.js` | Quads in draw order; the game's bitmap fonts |
| `src/interface/shapes.js` | `nineSlice()`, `wrapLines()`, `insideQuad()` |
| `src/interface/flashy.js` | `TitleIntro`: the "flashy" intro of a text |
| `src/interface/models.js` | The interface's 3D: its camera, material, logo and body scales |
| `src/interface/common-models.js` | The common hologram pack's models, in a battle or on a page without one |
| `src/menus/stack.js` | `ScreenStack`: the screens gone through, the pulse, the pointer, the hotspots |
| `src/menus/screen.js` | `MenuScreen`: widgets, focus, keys |
| `src/menus/widgets.js` | The widgets: `Rows`, `Options` (`OptionRow`), `Button`, `Text`, `Panel`, `List`, `Tabs`, `HorizontalButtons` |
| `src/menus/popup.js` | `confirmPopup()`: YES / NO, NO focused; `okPopup()` |
| `src/menus/input.js` | `bindMenuInput()`: the page's keys, pointer and wheel into a stack |
| `src/menus/sounds.js` | The interface sounds, through the game's sound events |
| `src/menus/options.js` | The options schema: every setting, its page, its choices, when a change applies |
| `src/menus/option-pages.js` | `optionsMenu()`: OPTIONS and its GAME, VIDEO, AUDIO and CONTROL pages |
| `src/menus/hotkeys.js` | The CONTROL page's key list, built from `src/input.js`'s bindings |
| `src/menus/themes/classic/` | `ClassicTheme`: how each widget looks (`index.js`) and the drawing underneath (`draw.js`) |
| `src/menus/themes/page/` | `PageTheme`: the same widgets as a page of elements in the front end's three looks (`index.js`, `style.js`). The looks themselves, the Saira font, the logo and the buttons' and titles' shapes are in `electron/ui/looks.css`, shared with the desktop launcher |
| `src/pause/index.js` | `createPauseMenu()`: the pause menu over a battle |
| `src/pause/overrides.js` | RESTART MISSION's URL carrying what the options changed |
| `src/menu.js` | `HtmlOptionsMenu`: the same options as a plain HTML form, behind `?menu=html` |
| `src/hud.js` | The HUD on the interface kit; its second layer slot (`setOverlay()`) |
| `src/settings.js` | URL overrides no longer saved |

## The interface kit

Everything is drawn in the original's interface space: 1024 units across
the screen and H = 768 + (h / w − 0.75) × 768 down it (768 at 4:3, 624 at
16:9), through one perspective camera onto a plane 886.81 units away, so a
group turned about its own axes shows in perspective. A group hangs from an
anchor: `surface.anchor(a, b, safe)` is the matrix from units at the screen
point (a, b), 0..1 across and down the safe area (90% of the screen, from
5% in) or the whole screen. `surface.toPlane(m, x, y)` is where a point of
the group shows; `surface.toInterface(px, py)` turns canvas pixels into
units.

`InterfaceSurface` is all a screen needs, with or without a battle:

```js
import { InterfaceSurface, commonTextureUrl } from "/src/interface/index.js";
const common = await fetch("/common/common.json").then((r) => r.json());
const surface = new InterfaceSurface(renderer, { url: commonTextureUrl(common), fonts: common.fonts, strings: common.strings });
// each frame, over whatever the canvas holds (a movie, the world):
surface.drawFrame((batch) => stack.draw(surface, batch));
```

- `surface.texture(name)`: a texture by its common.json name, loaded once
  and swapped for its 4x atlas when `public/common/raster-hud/` has one.
  Null for a name the table lacks.
- `surface.fonts`: `draw(batch, m, x, y, text, font, tint, align)`,
  `width()`, `lineHeight()`, `capsMiddle()`.
- `surface.text(key, ...args)`: a localized string with `%s` filled.
- `loadCommonModels({ common, texture })`: the hologram pack (the side
  logos, the shell_cube room) on a page with no battle.

The battle HUD (`BattleHud`) is an `InterfaceSurface` too; the spawn screen,
the scoreboard and the pause menu draw on it as layers.

## Screens and the stack

A `MenuScreen` is a page of widgets; a `ScreenStack` holds the screens gone
through and gives the top one the input.

```js
import { ScreenStack, MenuScreen, Rows, Button, ClassicTheme, bindMenuInput } from "/src/menus/index.js";
const stack = new ScreenStack({ theme: new ClassicTheme() });
const unbind = bindMenuInput({ stack, element: renderer.domElement, toInterface: (x, y) => surface.toInterface(x, y) });
stack.push(new MenuScreen({ id: "main", focus: "keep", back: null, widgets: [new Rows({ ... })] }));
// each frame: stack.update(dt); then draw it as above.
```

- `push(screen)` enters a screen forward; `pop()` goes back, and the
  screen under it comes up again with `shell_menu_exit`; `replace()`,
  `clear()`. `onEmpty` is called when the last screen is popped.
- **Entering** a screen (either way) replays its texts' intro and fades its
  items in over 0.4 s (`gShellAnimTime`). `focus: "first"` puts the focus on
  its first item each time (the OPTIONS menu); `"keep"` comes back to the
  item left from (the main menu).
- `MenuScreen` options: `id`; `widgets` (drawn in order); `back` (Esc and
  BACK: `"pop"`, a function, or null for none); `focus`; `dim` (the world
  or movie behind at half, as the option pages have it); `popup`; hooks
  `onEnter({ forward })`, `onExit()`, `onKey(code)`. Anything else given
  stays on the screen object.
- A screen may also have `draw(ctx, "under" | "over")` for its own quads
  around its widgets.

**Focus, as the original keeps it:**

- One focused item; hover and keyboard focus are the same state. The
  focused item is drawn lit and pulsing.
- Up and Down walk the screen's ring (its widgets' items, in order, those
  with `ring` true), wrapping; with nothing focused either goes to the
  first. `shell_select_change` plays on each move.
- Every pointer move gives the focus to what is under the pointer, or to
  nothing over empty space (then the next Up or Down starts from the
  first). Moving onto a new item plays `shell_select_change`.
- Enter or a click accepts the focused item; Left and Right change its value;
  Esc is the screen's `back`. The focused widget sees a key first (a list
  moves its own selection; a popup's buttons take Left and Right).
- The pulse: alpha 0.3 → 1 → 0.3 at 2 a second, one clock for all
  (`gButton_CurSizeAdd`). A list's selection frame pulses 1 → 0.5 → 1 once a
  second.

**Input.** `bindMenuInput({ stack, element, toInterface, scale, isActive,
exclusive, passKey })` binds the window's keys, the pointer over the canvas
(a captured mouse moves the drawn pointer by how far it moves) and the
wheel. `exclusive` takes key presses in the capture phase and stops them,
so nothing under the menus sees them (the pause menu over a battle); key
releases go on, so what tracks held keys underneath stays right. A pointer
move that didn't move (the browser sends one when it lets go of a captured
mouse) is ignored.
Directly: `stack.key(code)`, `stack.pointerMove(x, y)`,
`stack.pointerDown()`, `stack.pointerUp()`, `stack.wheel(deltaY)`.

**Sounds** go out as the game's `interface` sound event
(`src/audio/events.js`), by name: a battle's audio plays them from its
bank, the front end's from its own. `shell_select_change` (focus),
`shell_menu_accept` (an item chosen, a value changed, YES),
`shell_menu_cancel` (NO), `shell_menu_exit` (back to a screen),
`shell_menu_error`. Opening a popup is silent.

## Widgets

| Widget | What it is | Options |
| --- | --- | --- |
| `Rows` | The menu rows under an orange title bar, stacked into an arrow (main menu, SINGLEPLAYER, OPTIONS, GAME PAUSED, the era picker) | `title`; `rows: [{ id, label, hidden, disabled, action(screen) }]`; `layout: { at: [a, b], safe, x, y, containerX, rotX, rotY, rotZ, order, font, titleFont, width, ySpacing, yHeight, yTop }` (the original's `AddVerticalButtons` layout and its container; `x` is added to the shift the original gives the container, which it set over the container's own x, so leave it 0 to match; `containerX` replaces the shift); `listCount` (the original list's length, hidden rows included) |
| `Options` | An option page's label \| value rows: choices, toggles, sliders, actions | `rows` (`OptionRow`s); `layout`: `"game"`, `"video"`, `"audio"` or `"controls"` (the theme's look); `look` to override a look's numbers |
| `OptionRow` | One option row | `kind` (`"choice"`, `"toggle"`, `"slider"`, `"action"`, `"fixed"`), `label`, `get()`, `set(v)`, `choices: [[value, label]]`, `labels: [off, on]`, `min`, `max`, `step`, `format(v)`, `display` (an action's or fixed row's text), `note`, `action(screen)` |
| `Button` | BACK (bottom left), an accept-type button (bottom right: LAUNCH, OK), a middle one (RESET TO DEFAULTS) | `place`: `"back"`, `"accept"`, `"middle"`; `text`; `action(screen)` (BACK defaults to the screen's back); `disabled` |
| `Text` | A page title (`look: "title"`), a heading (`"heading"`), a popup's question (`"popup"`), or text anywhere (`at`, `x`, `y`, `font`, `align`, `color`) | `text`, `y`, `flashy` |
| `Panel` | A window, nine-sliced | `box: { x, y, w, h }` or `box(surface)`, `at`, `skin`, `look: "popup"` |
| `List` | A list box: the selected row's frame, the hovered row, the scroll bar | `rows` (lists of column texts, or a function), `look` (`{ at, box, font, pitch, columns: [{ x, align }] }` or a function of the surface), `onSelect`, `onAccept` (Enter, or a double click within 0.4 s) |
| `Tabs` | Header-bar tabs (the CONTROL page's) | `tabs: [{ id, label }]`, `active`, `onChange(index, screen)`, `look` |
| `HorizontalButtons` | Buttons side by side (a popup's YES / NO) | `buttons: [{ id, label, action(screen), sound }]` |

Every widget has `id`, `hidden`, `ring` and `disabled`. A new kind is a
`Widget` subclass (its `items()`, `activate()`, `step()`, `key()`, `press()`,
`drag()`, `wheel()`, `hover()` as it needs) with a painter registered on
the theme: `theme.register(kind, (widget, ctx) => { ... })`. The painter
draws into `ctx.batch` on `ctx.surface`, reads `ctx.alpha` (the screen's
fade), `ctx.pulse`, `ctx.focused(widget, item)` and `ctx.screen.intro(key,
delay)`, and records what the pointer can hit with
`ctx.hotspot(target, corners)` (corners from `toPlane`, so tilted things
hit-test as drawn).

**Popups.** `confirmPopup({ message, onYes, onNo, strings })` is a screen
to push: a 300 × 180 window at the screen's centre, the question in
gamefont_medium over YES and NO, NO focused. YES plays `shell_menu_accept`,
NO and Esc `shell_menu_cancel`; either closes it first. The screen under it
keeps its title and corner buttons and hides its rows, lists and options.
`okPopup({ message, onOk, strings })` is the original's Popup_Ok: 260 × 180,
the message over OK.

## The Classic theme

`ClassicTheme` draws each kind as the original does:

- **Rows.** The container is moved left by half the widest bar its whole
  list would have (hidden rows count: the pause menu's list has 11, so it
  shifts by 200); rows are a pitch (font height + ySpacing) apart, centred
  on the container; bar widths grow a pitch a row down to the middle and
  shrink after, and the rows below the middle have their right ends
  flipped, which makes the arrow. The title bar is 1.75 pitches over the
  first row, half a pitch in. A row's bar is the pitch tall, starting 4.7
  units left of the row; the title's runs from a quarter pitch over its
  text to a quarter pitch under its glyphs' full depth (deeper than the
  font's line, as the captures measure).
  Unfocused bars are at 150/255; the focused row's bar and text take the
  pulse's alpha. Each row's intro starts 0.1 s after the one above.
- **Bars** are `buttonleft` / `buttonmid` / `buttonright` (blue) or
  `headerbutton*` (orange): a left end as wide as the bar is tall, the
  middle stretched, the right end past the bar's width.
- **Text without a bar** that has the focus (option values, corner buttons,
  YES / NO) is yellow, its green following its alpha: (255, 1.2 A + 50
  clamped to 100..255, 100).
- **Option pages** dim the world to half; labels end 15 units left of the
  centre, values start 15 right (the GAME and VIDEO pages), or the AUDIO
  page's: labels to 0.4 of the safe width, sliders from there to 176.8
  units short of the right edge, the value at the right. A row's capitals
  are centred on its line. Sliders: `slider_bg` with a `slider_fg` thumb a
  tenth of the width; the focused one solid, the others at half alpha.
  Rows that don't fit scroll with the focus, with a scroll bar.
- **Lists**: `listbox_cursor` around the selection, `listbox_hilight` on
  the hovered row, `slider_bg` / `slider_fg` see-through for the scroll bar.
- **Buttons**: gamefont_medium, their capitals' middle 12 units over the
  safe area's bottom, 21 in from its side.
- **The pointer**: `mousecursor`, over everything, once the pointer has
  moved or a screen sets `stack.pointer.visible` (the pause menu starts it
  in the middle, where the original warps the mouse);
  `stack.showPointer = false` hides it.

**Another theme** (an Enhanced one) is any object with
`draw(stack, surface, batch)`, which draws `stack.visible` and fills
`stack.hotspots` with `{ target, corners }`, and `hit(stack, x, y)`; the
widgets, screens, focus, input and sounds stay as they are. The pause menu
takes it as `createPauseMenu({ theme })`.

**The Page theme** (`PageTheme`, the front end's own, a design in
progress) draws no canvas: `render(stack)`, each frame, keeps a
page of elements in step with the same screens and widgets (the focus,
values, labels, rows that came or went), in one of three looks that share
the layout (`bf1`, `bf2`, `eternal`; the `menuTheme` setting). Keys still
go to the stack (`bindMenuInput({ pointer: false })`); the pointer works on
the page's own elements through the screens' `setFocus`, `accept` and
`step`, so the sounds and actions are the screens' own. A screen's widgets
go by kind (its title at the top, rows, option rows, lists and tabs down
the middle, BACK, middle and accept buttons along the bottom, a popup in a
window over the screen under it) unless the shell's layouts say otherwise
(the main menu under the logo; Instant Action's columns); a kind the page
doesn't know is drawn by the shell's `views` (the server browser). Moving
between screens is choreographed: the screen left cascades out, the next
cascades in from the side it is reached from, bottom buttons both have
stay put, the logo flies between the main menu and the corner, and each
screen leaves the page when its own animations are done (a slow frame
can't cut them short). The looks are custom properties on
`<html data-bfe-theme>`, which the front end's dialogs take too. Over a
battle the pause menu uses it as an overlay (`new PageTheme({ overlay:
true })`, `src/game.js`): no backdrop, the frozen world darkened and
blurred behind its screens, the system's pointer on its elements, gone
while it is closed but for CLICK TO RESUME (`prompt()`); the HUD's
overlay slot drives it (`draw()` renders the page). `?menus=classic`
keeps the original's pause menu on the HUD. The battle's audio panel and
chat take the look too.

Textures not extracted yet fall back to stand-ins drawn in code to the same
shapes (`buttonleft`, `buttonmid`, `buttonright`, `slider_bg`, `slider_fg`,
`white`) or flat colours, so screens read the same until the art is there.
The original swaps a hovered window's skin to `border_3a_pieces`; the
captures don't show it, so the theme doesn't.

## Options

`src/menus/options.js` lists every setting of `src/settings.js` once, with
its page, label (the original's string where it had one), kind, choices or
range, and when a change applies:

| Applies | Settings |
| --- | --- |
| at once (`live`) | everything else |
| when a map next loads (`map`) | compressed textures |
| when a battle starts (`battle`) | the Classic AI's mode, difficulty and heroes, the battle size; RESTART MISSION brings them in |
| never (`fixed`) | the simulation rate (60 Hz) |

A focused row that doesn't apply at once says so under the rows.

`optionsMenu({ settings, strings, inBattle, viewpoint, audioPanel, online,
credits, integrations })` is the OPTIONS screen (rows turned 40 degrees: GAME,
VIDEO, AUDIO, CONTROL OPTIONS; the front end passes `online` and `credits` to
show those rows, and the desktop client's front end `integrations` for STEAM &
DISCORD, [presence.md](presence.md)) and opens the pages:

- **GAME OPTIONS:** VIEWPOINT (in a battle: `viewpoint`), FRIENDLY FIRE,
  INSTANT ACTION HEROES, DIFFICULTY, BATTLE AI, BATTLE SIZE, STATS LINE,
  SIMULATION RATE, RESET TO DEFAULTS.
- **VIDEO OPTIONS:** the quality preset, frame limit, adaptive resolution,
  render scale, upscaler and sharpening, antialiasing, shadows, texture
  filtering, compressed textures, water, view distance, far terrain, bump
  mapping, soft focus, bloom, motion blur, soft particles, lamp light; then
  each effect's CLASSIC or ENHANCED (terrain, trees, water and steam, ocean
  and sky, rain, blaster bolts, jetpacks, droidekas, destruction, death
  camera). They scroll. RESET TO DEFAULTS in the middle (the original's AUTO
  DETECT place).
- **AUDIO OPTIONS:** MASTER, MUSIC, SFX, SPEECH, AMBIENCE and INTERFACE
  VOLUME sliders; SOUND (ENHANCED or CLASSIC), OUTPUT MODE, DYNAMICS, MONO,
  CLARITY; SPEAKER TEST (the audio panel: speaker test and the mix, where a
  battle's sound has loaded); RESET TO DEFAULTS.
- **CONTROL OPTIONS:** below.

Each change is saved at once (`Settings.set`). RESET TO DEFAULTS resets that
page's settings.

## The CONTROL page and the keys

In place of the line of directions that used to sit at the bottom of the
screen, the CONTROL OPTIONS page lists every key, as the original's
`ifs_opt_pccontrols` does: MOUSE SENSITIVITY (0.5 to 10, in steps of 0.1)
and INVERT Y AXIS in a window, then CONFIG BINDINGS: INFANTRY, VEHICLE and
STARFIGHTER tabs over an "ACTION: | KEYS" list, several keys joined with ", "
in the original's key names (MOUSE BUTTON 1, SPACE, CONTROL, MOUSE WHEEL
UP...).

- The rows come from `src/input.js`'s `BINDINGS` (axes, presses, holds), so
  they follow them; keys the game reads directly (vehicle brake and climb,
  the jetpack, the dive, Tab's table, Esc) are listed as they are.
- Up and Down run from the two rows into the list and back; Left and Right
  in the list, or a click, change the tab; Page Up / Page Down and the wheel
  page and scroll it.
- **Rebinding.** Enter or a double click on a bound row opens the original's
  PRESS NEW KEY, ESC TO CANCEL popup (`keyPopup()`, `src/menus/popup.js`;
  `ifs.GameOpt.pc_keybindpopup`) with the action over it. The next key,
  mouse button or wheel notch becomes that action's only key and leaves any
  other action of the same set that had it, so a key does one thing
  (`rebind()`, `src/input.js`). Esc cancels. The keys the game reads itself
  (Esc, Tab, T and Y), the window's own (F11, F12) and keys outside the
  network's set (`INPUT_CODES`) are refused, and the popup waits on. It works the same in the front end
  and over a battle: keys come through the menu stack, and the mouse is
  heard on the window ahead of the menus while the popup is open.
- The changes are saved at once as settings, only where they differ from
  the defaults: `keyBindings` for INFANTRY and VEHICLE (one set: the rows
  carry `binding: "presses.jump"`, `"holds.firePrimary"`, `"axes.forward+"`)
  and `flightBindings` for STARFIGHTER. RESET TO DEFAULTS clears both.
- **The world never sees the player's keys.** `Controls` reads presses,
  holds and axes through the player's bindings, and each command's held
  keys go as the world's own (`worldKeys`): a held hold or axis side as its
  table's first key (MOVE FORWARD as W, PRIMARY FIRE as MOUSE BUTTON 1;
  TURN LEFT and RIGHT, unbound by default, as keypad * and /,
  `WORLD_AXES`), a starfighter's controls as `FLIGHT_BINDINGS`' first keys
  with `flightSettings.bindings` left `{}`, and keys bound in their place
  left out. So the page, a world worker, a replay and the server read one
  set of keys, and the network's single binding profile carries every
  player's bindings.

## The pause menu

`createPauseMenu()` (`src/pause/index.js`), as `ifs_pausemenu`:

- **Opening.** Esc, or the browser letting go of the mouse while the game
  had it (Esc pressed while it was captured, the page losing focus):
  `pointerlockchange` with no release of the game's own
  (`Controls.release()`, which the spawn screen uses). Over the spawn screen
  and the scoreboard their own Esc opens it (`setEscapeOwners()`).
- **What shows.** GAME PAUSED over RESUME GAME, OPTIONS, RESPAWN, RESTART
  MISSION, QUIT and EXIT TO WINDOWS, turned 25 degrees at the safe area's
  (0.6, 0.5), 20 units down; BACK at the bottom left. Nothing is dimmed.
  RESUME GAME is focused each time it opens.
- **The world** stops (the fixed clock doesn't advance) offline. In a
  network game the server's battle plays on behind the menu, as the
  original's network games did: the page keeps receiving and drawing it,
  and his soldier stands still until he resumes: `stillCommand` sends no
  keys or look and a centred gamepad, so the server never sees a gap in
  his input and a stick held into the menu stops flying his craft (a
  command without a pad leaves the server holding the last one). He can
  be shot meanwhile, and he
  hears it: the game's sound plays on too. Either way the HUD hides (the
  menu is the HUD's overlay slot, `hud.setOverlay()`). Offline the game's
  sound fades out over 0.25 s (`GameAudio.setPaused()`: the `ingame` bus,
  back to where it was after). The audio's own clock keeps running, so the
  menu's sounds (on `shellfx`) play and repeat; before, a paused frame
  stopped the engine's clock and a sound's play interval never ran out.
- **Over a screen.** Over the spawn screen only its 3D shows (the room and
  the class's body, as the original's capture shows); over the scoreboard
  the board hides. Closing comes back to that screen with the mouse free.
- **Choices**, each but RESUME and OPTIONS asked first (YES / NO, NO
  focused):

  | Row | Question (string key) | YES |
  | --- | --- | --- |
  | RESPAWN | `game.pause.suicide_prompt` | kills the soldier (when alive) and resumes |
  | RESTART MISSION | `ifs.pause.warn_restart` | the battle's URL again (`src/launch.js` `restartUrl()`), its setting overrides rewritten to the options' current values |
  | QUIT | `ifs.pause.warn_host_quit` | the front end's main menu (`mainMenuUrl()`) |
  | EXIT TO WINDOWS | `ifs.main.askquit` | `window.close()`, then the main menu if the browser keeps the tab |

- **RESUME** (the row, BACK or Esc) asks for the mouse again from inside
  the click or key that chose it. Chrome refuses a capture right after Esc
  let the mouse go; then the world stays paused, the HUD comes back with
  CLICK TO RESUME, and the next click captures it and plays on. Esc there
  opens the menu again. A second Esc at the pause menu's root resumes when
  the key is released, so the browser doesn't read that press as an
  immediate release of the new capture; Esc on a nested options page still
  goes back one page.
- RESPAWN is listed always, as in the original; its YES does nothing to a
  soldier who isn't alive.
- Leaving (RESTART MISSION, QUIT, EXIT TO WINDOWS) replaces the page, as the
  end table's next battle does, so the browser's Back doesn't lead into a
  battle again.

The object returned is what the spawn screen and scoreboard already call:
`isOpen`, `open(why)`, `close(resume)`, `onOpen`, `onClose` (they wrap it),
`enableRespawn({ label, prompt, available, respawn })`, plus `stack`,
`awaitPointer(on)`, `setEscapeOwners()`, `update(dt)` with `clock: "manual"`,
`leaving` (the URL a choice went to), and `navigate(url)` / `closeWindow()`,
which a check can replace.

**Esc on the scoreboard** now opens the pause menu over the table, Tab's or
the end of the battle's; on DETAILS and AWARDS it still goes back to the
table. DONE and the 30 s idle still go on to the next battle.

## What went

- The settings button and the HTML settings dialog (`#settings`) and their
  CSS. `?menu=html` brings a plain HTML form of the same schema back
  (`src/menu.js`), opened by Esc.
- The line of directions in `#hud` and the "O: settings" key. `#hud` stays,
  empty, for load errors (the tests wait on its "Failed" text).
- Saving URL overrides: `?ai=classic` and the like apply for the visit and
  are no longer written to `battlefront-eternal.settings` when any setting
  changes (`Settings.saved` holds what is saved). Setting one of them by hand
  saves it.

## Not the original's

- **What the pages hold** is this port's: its rendering, audio, AI and
  game settings in the original's page layouts. The VIDEO page has more
  rows than the original's 16, so it scrolls, with a list's scroll bar.
  RESET TO DEFAULTS stands where the original's AUTO DETECT was; the
  CONTROL page has no GAMEPAD CONTROL (the port has no gamepad input), and
  the AUDIO page none of the original's EAX logo or mixer rows.
- **In a battle** the GAME page keeps FRIENDLY FIRE and INSTANT ACTION
  HEROES (the original hides them there); the ones that wait for a new
  battle say so, and RESTART MISSION brings them in.
- **EXIT TO WINDOWS** closes the tab when the browser allows it, else goes
  to the main menu.
- **CLICK TO RESUME** is the port's: a browser can refuse to capture the
  mouse again at once.
- **Esc on the scoreboard's end table** opens the pause menu (the
  original's Esc was DONE there).
- **Not implemented:** the original's P screenshot key and its multiplayer
  communications macros.
- **The hover skin** (`border_3a_pieces`, orange) isn't drawn: the
  captures never show it.
- **Measured against the captures** (every capture from the first
  level load on, the pause and options screens included, is 12 px low),
  the stacks' rows sit within 1 to 3 pixels of the layout's places (the
  captures' row pitch is about 2% under the layout's); the layout's numbers
  are kept. The VIDEO page's gamefont_medium and its 110-unit sliders, the
  title bars' depth, the option rows' text on their lines, the bottom
  buttons and the controls page's tabs and window are as the captures
  measure.

## Checks

```sh
node --test tests/menus-stack.test.js tests/menus-options.test.js tests/menus-hotkeys.test.js tests/settings.test.js tests/pause-menu.test.js
PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/pause-menu-browser.mjs
```

**The node tests cover:** the stack (push, pop, popups, `onEmpty`), focus
and navigation (the ring, wrapping, the pointer taking and dropping the
focus, Enter and clicks, sounds), the intro and fade on entry, the pulse
frame by frame, option rows (choices, toggles, sliders, a slider dragged,
scrolling), lists (selection, handing the focus on, double clicks, the
wheel), tabs, the row stacks' geometry against the original's layout
numbers; the schema (every setting of `DEFAULTS` on a page, each saying when
it applies, defaults among the choices), the pages' rows, the CONTROL page's
tabs; the key list from the bindings (and a changed binding showing);
rebinding (`tests/key-bindings.test.js`: a key moved from the binding that
had it, the popup through the stack with a key, a mouse button, the wheel,
Esc and refused keys, the rest of a taken click swallowed, no key reaching
the browser meanwhile, RESET TO DEFAULTS, and the world's keys a rebound
command carries, starfighter's included, encoded for the network); the
settings leak; the pause menu's rows, questions, YES and NO, its URLs and
the escape owners.

**The browser check**, on geo1, at 1024 × 768 and 1280 × 720, with real
keys and clicks: Esc pauses (the world still, the HUD hidden, the sound
paused), RESUME plays on, a refused capture shows CLICK TO RESUME, OPTIONS
and every page (a change made and saved, the CONTROL tabs), RESPAWN,
RESTART MISSION, QUIT and EXIT TO WINDOWS through their questions, the
menu over the spawn screen and over the scoreboard (Tab's table and the
end table), and the menus on a page with no battle (`tests/menus-alone.html`).
It writes each screen into the folder `MENUS_SHOTS` names and, where
`MENUS_LIVE` names a folder of captures of the 2004 game, beside the matching
capture (`sidebyside-<screen>-<size>.png`).

## Chat

`src/chat.js` handles text entry and a bounded feed. T opens global chat
(yellow), Y team chat (green), Enter sends, and Escape cancels. Text entry
clears and disables gameplay input. Messages are rendered as text, limited to
240 characters, with eight recent lines and a twelve-second visibility
period. Open chat shows the retained history.

Offline, a sent message echoes locally through `chat.send(message)`. In an
online battle the page replaces that method with the network session's
(`client.host.chat(message)`), and the messages the server accepts and routes
(global, team and server announcements) arrive through
`chat.receive({channel, name, text})`. Blue announcements use
`chat.server(text)` (available for testing as `__bf.chat.server(text)`).
