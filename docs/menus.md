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

| Crate, module | What it does |
| --- | --- |
| `rust/bfe-interface` (`space`) | The interface space: 1024 x H units, the perspective plane, `anchor_matrix`, `plane_point`, `to_interface` |
| `rust/bfe-interface` (`surface`) | `InterfaceSurface` and `Surface`: textures by their common.json name, the fonts, the anchors, pointer conversion |
| `rust/bfe-interface` (`fonts`) | The game's bitmap fonts: measuring and laying out glyph quads |
| `rust/bfe-interface` (`shapes`) | `nine_slice`, `wrap_lines`, `inside_quad` |
| `rust/bfe-interface` (`flashy`) | `TitleIntro`: the "flashy" intro of a text |
| `rust/bfe-interface` (`stack`, `screen`) | `ScreenStack`: the screens gone through; `MenuScreen`: widgets, focus, keys |
| `rust/bfe-interface` (`widgets`) | The widgets: rows, option rows, buttons, text, panels, lists, tabs, horizontal buttons |
| `rust/bfe-interface` (`popup`) | `confirm_popup` (YES / NO, NO focused), `ok_popup`, `key_popup` |
| `rust/bfe-interface` (`input`) | `MenuInput`: keys, pointer and wheel into a stack |
| `rust/bfe-interface` (`sounds`) | The interface sounds, by the game's sound names |
| `rust/bfe-interface` (`options`, `settings`) | The options schema (every setting, its page, its choices, when a change applies) and the saved settings |
| `rust/bfe-interface` (`option_pages`) | OPTIONS and its GAME, VIDEO, AUDIO and CONTROL pages; `OptionsController` |
| `rust/bfe-interface` (`hotkeys`) | The CONTROL page's key lists, the default bindings, rebinding |
| `rust/bfe-interface` (`theme`) | The Classic look's colours, row geometry and drawing primitives, and the pointer's hit regions |
| `rust/bfe-ui` | The front end's own look: the Slint screens (main menu, Instant Action, multiplayer, options, first-run setup, battle screens) in its three looks |
| `rust/bfe-client` (`app/slint_*`) | Keeps those screens in step with the Rust menu state and routes their callbacks back to it |
| `rust/bfe-battle-ui` (`pause`) | `PauseMenu`: the pause menu over a battle |
| `rust/bfe-battle-ui` (`chat`) | The chat log and its input |

## The interface kit

Everything is drawn in the original's interface space: 1024 units across
the screen and H = 768 + (h / w - 0.75) x 768 down it (768 at 4:3, 624 at
16:9), through one perspective camera onto a plane 886.81 units away, so a
group turned about its own axes shows in perspective. A group hangs from an
anchor: `anchor_matrix(a, b, safe, height)` is the matrix from units at the
screen point (a, b), 0..1 across and down the safe area (90% of the screen,
from 5% in) or the whole screen. `plane_point` is where a point of the group
shows; `to_interface(px, py, ...)` turns window pixels into units.

Textures are named as in `common.json`, and a texture is swapped for its 4x
atlas where `common/raster-hud/` has one. The fonts measure and draw the
game's bitmap fonts (`width`, line height, the middle of the capitals);
strings are the game's localized strings with `%s` filled.

The battle HUD, the spawn screen, the scoreboard and the pause menu draw on
the same interface space.

## Screens and the stack

A `MenuScreen` is a screen of widgets; a `ScreenStack` holds the screens
gone through and gives the top one the input.

- `push(screen)` enters a screen forward; `pop()` goes back, and the
  screen under it comes up again with `shell_menu_exit`; `replace()`,
  `clear()`.
- **Entering** a screen (either way) replays its texts' intro and fades its
  items in over 0.4 s (`gShellAnimTime`). `ScreenFocus::First` puts the
  focus on its first item each time (the OPTIONS menu); `Keep` comes back
  to the item left from (the main menu).
- A screen has an id, its widgets (drawn in order), what Esc and BACK do
  (go back, an action, or nothing), its focus rule, whether it dims the
  world or movie behind it (the option pages do) and whether it is a popup.

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

**Input.** `MenuInput` takes the window's keys, the pointer (a captured
mouse moves the drawn pointer by how far it moves) and the wheel, and turns
them into focus moves, value steps, slider drags and widget actions on the
top screen. Over a battle the menus take every key press while they are
open, so nothing under them sees it; key releases go on, so what tracks
held keys underneath stays right.

**Sounds** go out as the game's `interface` sound event
(`rust/audio-control`), by name: a battle's audio plays them from its
bank, the front end's from its own. `shell_select_change` (focus),
`shell_menu_accept` (an item chosen, a value changed, YES),
`shell_menu_cancel` (NO), `shell_menu_exit` (back to a screen),
`shell_menu_error`. Opening a popup is silent.

## Widgets

| Widget | What it is |
| --- | --- |
| `Rows` | The menu rows under an orange title bar, stacked into an arrow (main menu, SINGLEPLAYER, OPTIONS, GAME PAUSED, the era picker). Its layout is the original's `AddVerticalButtons` layout and its container (anchor, offsets, rotation, font, spacing); `list_count` is the original list's length, hidden rows included |
| `Options` | An option page's label \| value rows: choices, toggles, sliders, actions; laid out as the GAME, VIDEO, AUDIO or CONTROL page |
| `OptionRow` | One option row: a choice, toggle, slider, action or fixed value, with its note |
| `Button` | BACK (bottom left), an accept-type button (bottom right: LAUNCH, OK), a middle one (RESET TO DEFAULTS) |
| `Text` | A page title, a heading, a popup's question, or text anywhere, with or without the "flashy" intro |
| `Panel` | A window, nine-sliced (`border_3_pieces`) |
| `List` | A list box: the selected row's frame, the hovered row, the scroll bar; Enter or a double click within 0.4 s accepts |
| `Tabs` | Header-bar tabs (the CONTROL page's) |
| `HorizontalButtons` | Buttons side by side (a popup's YES / NO) |

Every widget can be hidden, left out of the focus ring, or disabled
(drawn grey). Choosing an item gives a `WidgetAction` (open a page, go
back, reset a page, start a rebind, select a tab, or a named action) that
the screen's owner carries out.

**Popups.** `confirm_popup` is a screen to push: a 300 x 180 window at the
screen's centre, the question in gamefont_medium over YES and NO, NO
focused. YES plays `shell_menu_accept`, NO and Esc `shell_menu_cancel`;
either closes it first. The screen under it keeps its title and corner
buttons and hides its rows, lists and options. `ok_popup` is the
original's Popup_Ok: 260 x 180, the message over OK.

## The Classic look

The Classic look is the original's, and its numbers (`rust/bfe-interface`
`theme`) are the reference every menu's layout is measured against:

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
  moved (the pause menu starts it in the middle, where the original warps
  the mouse).

**The front end's own look** (a design in progress) is drawn by Slint
(`rust/bfe-ui`): the same screens and widgets, kept in step with the Rust
menu state every frame (the focus, values, labels, rows that came or went)
by `rust/bfe-client` (`app/slint_*`), in one of three looks that share the
layout (`bf1`, `bf2`, `eternal`; the `menuTheme` setting). Keys still go to
the screen stack; the pointer works on the drawn elements, which call back
into the screens' focus, accept and step, so the sounds and actions are the
screens' own. A screen's widgets go by kind (its title at the top, rows,
option rows, lists and tabs down the middle, BACK, middle and accept
buttons along the bottom, a popup in a window over the screen under it)
unless the front end's layouts say otherwise (the main menu under the logo;
Instant Action's columns; the server browser). Moving between screens is
choreographed: the screen left cascades out, the next cascades in from the
side it is reached from, bottom buttons both have stay put, the logo flies
between the main menu and the corner, and each screen leaves when its own
animations are done (a slow frame can't cut them short). Over a battle the
pause menu, the spawn screen and the scoreboard take the same look: no
backdrop, the world darkened and blurred behind their screens
(`app/page_blur.rs`), the system's pointer on their elements. The battle's
audio panel and chat take the look too.

Textures not extracted yet fall back to stand-ins drawn in code to the same
shapes (`buttonleft`, `buttonmid`, `buttonright`, `slider_bg`, `slider_fg`,
`white`) or flat colours, so screens read the same until the art is there.
The original swaps a hovered window's skin to `border_3a_pieces`; the
captures don't show it, so the theme doesn't.

## Options

`rust/bfe-interface` (`options`) lists every setting once, with
its page, label (the original's string where it had one), kind, choices or
range, and when a change applies:

| Applies | Settings |
| --- | --- |
| at once (`live`) | everything else |
| when a map next loads (`map`) | compressed textures |
| when a battle starts (`battle`) | the Classic AI's mode, difficulty and heroes, the battle size; RESTART MISSION brings them in |
| never (`fixed`) | the simulation rate (60 Hz) |

A focused row that doesn't apply at once says so under the rows.

`options_menu` (`rust/bfe-interface` `option_pages`) is the OPTIONS screen
(rows turned 40 degrees: GAME, VIDEO, AUDIO, CONTROL OPTIONS; in the front
end also the online and credits rows and STEAM & DISCORD,
[presence.md](presence.md)) and opens the pages:

- **GAME OPTIONS:** VIEWPOINT (in a battle: `viewpoint`), FRIENDLY FIRE,
  INSTANT ACTION HEROES, DIFFICULTY, BATTLE AI, BATTLE SIZE, STATS LINE,
  SIMULATION RATE, RESET TO DEFAULTS.
- **VIDEO OPTIONS:** the quality preset (its note names the GPU, and warns
  when the graphics driver draws in software), frame limit, adaptive resolution, render scale (0.25 to 2; its
  note the 3D view's size), upscaler (its note what each costs) and
  sharpening, antialiasing, shadows, texture filtering, compressed
  textures, water, view distance (SHORT, ORIGINAL, FAR, MAXIMUM), far terrain, bump mapping, soft focus,
  bloom, motion blur, soft particles, lamp light; then each effect's
  CLASSIC or ENHANCED (terrain, trees, water and steam, ocean
  and sky, rain, blaster bolts, jetpacks, droidekas, destruction, death
  camera). They scroll. RESET TO DEFAULTS in the middle (the original's AUTO
  DETECT place).
- **AUDIO OPTIONS:** MASTER, MUSIC, SFX, SPEECH, AMBIENCE and INTERFACE
  VOLUME sliders; SOUND (ENHANCED or CLASSIC), OUTPUT MODE, DYNAMICS, MONO,
  CLARITY; SPEAKER TEST (the audio panel: speaker test and the mix, where a
  battle's sound has loaded); RESET TO DEFAULTS.
- **CONTROL OPTIONS:** below.

Each change is saved at once to `player-settings.json` in the user data
folder. RESET TO DEFAULTS resets that page's settings.

## The CONTROL page and the keys

The CONTROL OPTIONS page lists every key, as the original's
`ifs_opt_pccontrols` does: MOUSE SENSITIVITY (0.5 to 10, in steps of 0.1)
and INVERT Y AXIS in a window, then CONFIG BINDINGS: INFANTRY, VEHICLE and
STARFIGHTER tabs over an "ACTION: | KEYS" list, several keys joined with ", "
in the original's key names (MOUSE BUTTON 1, SPACE, CONTROL, MOUSE WHEEL
UP...).

- The rows come from the bindings (`rust/bfe-interface` `hotkeys`: axes, presses, holds), so
  they follow them; keys the game reads directly (vehicle brake and climb,
  the jetpack, the dive, Tab's table, Esc) are listed as they are.
- Up and Down run from the two rows into the list and back; Left and Right
  in the list, or a click, change the tab; Page Up / Page Down and the wheel
  page and scroll it.
- **Rebinding.** Enter or a double click on a bound row opens the original's
  PRESS NEW KEY, ESC TO CANCEL popup (`key_popup`;
  `ifs.GameOpt.pc_keybindpopup`) with the action over it. The next key,
  mouse button or wheel notch becomes that action's only key and leaves any
  other action of the same set that had it, so a key does one thing
  (`rebind`). Esc cancels. The keys the game reads itself
  (Esc, Tab, T and Y), the window's own (F11, F12) and keys outside the
  network's set (`INPUT_CODES`) are refused, and the popup waits on. It works the same in the front end
  and over a battle: keys and the mouse come through the menu stack while
  the popup is open.
- The changes are saved at once as settings, only where they differ from
  the defaults: `keyBindings` for INFANTRY and VEHICLE (one set: the rows
  carry `binding: "presses.jump"`, `"holds.firePrimary"`, `"axes.forward+"`)
  and `flightBindings` for STARFIGHTER. RESET TO DEFAULTS clears both.
- **The world never sees the player's keys.** The client reads presses,
  holds and axes through the player's bindings, and each command's held
  keys go as the world's own: a held hold or axis side as its
  table's first key (MOVE FORWARD as W, PRIMARY FIRE as MOUSE BUTTON 1;
  TURN LEFT and RIGHT, unbound by default, as keypad * and /,
  `WORLD_AXES`), a starfighter's controls as `FLIGHT_BINDINGS`' first keys
  with `flightSettings.bindings` left `{}`, and keys bound in their place
  left out. So the client, the local world thread, a replay and the server
  read one set of keys, and the network's single binding profile carries every
  player's bindings.

## The pause menu

`PauseMenu` (`rust/bfe-battle-ui` `pause`), as `ifs_pausemenu`:

- **Opening.** Esc, or the game window losing focus while a battle runs
  (the client then also lets go of the keys held). Over the spawn screen
  and the scoreboard their own Esc opens it.
- **What shows.** GAME PAUSED over RESUME GAME, OPTIONS, RESPAWN, RESTART
  MISSION, QUIT and EXIT TO WINDOWS, turned 25 degrees at the safe area's
  (0.6, 0.5), 20 units down; BACK at the bottom left. Nothing is dimmed.
  RESUME GAME is focused each time it opens. Online, RESTART MISSION is
  hidden and PLAYER LIST is shown.
- **The world** stops in single player: the local world thread is paused
  (the fixed clock doesn't advance) and its frame, darkened and blurred, is
  drawn once and shown again until something changes it. In a network game
  the server's battle plays on behind the menu, as the original's network
  games did: the client keeps receiving and drawing it, and the player's
  soldier stands still until they resume: the client sends a still command
  every tick (no keys or look and a centred gamepad), so the server never
  sees a gap in the input and a stick held into the menu stops flying the
  craft. The player can be shot meanwhile, and hears it: the game's sound
  plays on too. Either way the HUD hides. Offline the game's sound fades out
  over 0.25 s and comes back after. The audio's own clock keeps running, so
  the menu's sounds play and repeat.
- **Over a screen.** Over the spawn screen only its 3D shows (the room and
  the class's body, as the original's capture shows); over the scoreboard
  the board hides. Closing comes back to that screen with the mouse free.
- **Choices**, each but RESUME and OPTIONS asked first (YES / NO, NO
  focused):

  | Row | Question (string key) | YES |
  | --- | --- | --- |
  | RESPAWN | `game.pause.suicide_prompt` | kills the soldier (when alive) and resumes |
  | RESTART MISSION | `ifs.pause.warn_restart` | the same playlist entry again from tick 0, with the options' current values |
  | QUIT | `ifs.pause.warn_host_quit` | the front end's main menu |
  | EXIT TO WINDOWS | `ifs.main.askquit` | closes the game |

- **RESUME** (the row, BACK or Esc) captures the mouse again and plays on.
  Esc on a nested options page goes back one page.
- RESPAWN is listed always, as in the original; its YES does nothing to a
  soldier who isn't alive.

**Esc on the scoreboard** opens the pause menu over the table, Tab's or
the end of the battle's; on DETAILS and AWARDS it still goes back to the
table. DONE and the 30 s idle still go on to the next battle.

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
- **Esc on the scoreboard's end table** opens the pause menu (the
  original's Esc was DONE there).
- **Not implemented:** the original's P screenshot key and its multiplayer
  communications macros.
- **The hover skin** (`border_3a_pieces`, orange) isn't drawn: the
  captures never show it.
- **Measured against the captures** (every capture from the initial
  level load on, the pause and options screens included, is 12 px low),
  the stacks' rows sit within 1 to 3 pixels of the layout's places (the
  captures' row pitch is about 2% under the layout's); the layout's numbers
  are kept. The VIDEO page's gamefont_medium and its 110-unit sliders, the
  title bars' depth, the option rows' text on their lines, the bottom
  buttons and the controls page's tabs and window are as the captures
  measure.

## Checks

```sh
cd rust
cargo test --locked -p bfe-interface -p bfe-battle-ui -p bfe-shell
```

The unit tests cover the stack (push, pop, popups), focus and navigation
(the ring, wrapping, the pointer taking and dropping the focus, Enter and
clicks, sounds), the intro and fade on entry, the pulse, option rows
(choices, toggles, sliders, a slider dragged, scrolling), lists (selection,
double clicks, the wheel), tabs, the row stacks' geometry against the
original's layout numbers; the schema (every setting on a page, each saying
when it applies, defaults among the choices), the pages' rows, the CONTROL
page's tabs; the key list from the bindings; rebinding (a key moved from
the binding that had it, a mouse button, the wheel, Esc and refused keys,
RESET TO DEFAULTS); the pause menu's rows, questions, YES and NO.

## Chat

`rust/bfe-battle-ui` (`chat`) handles text entry and a bounded feed. T opens
global chat (yellow), Y team chat (green), Enter sends, and Escape cancels.
Text entry clears and disables gameplay input. Messages are drawn as text,
limited to 240 characters, with eight recent lines and a twelve-second
visibility period. Open chat shows the kept history.

Offline, a sent message echoes locally. In an online battle it goes to the
server, and the messages the server accepts and routes (global, team and
server announcements) come back to every player it concerns. Server
announcements are blue.
