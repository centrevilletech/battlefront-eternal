# The scoreboard

Both teams' Characters, bots included, with their KILLS, DEATHS and CP
POINTS, as the original's end-of-match table lists them, and what that
table leads to: DETAILS (one Character's record) and AWARDS. Also the stat
popup in play. It has two sides here:

- **The board** (`src/scoreboard/`): what is drawn and how input moves it.
  It reads the world only through the scoreboard port.
- **The world side** (`src/battle.js`, `src/battle-stats.js`,
  `src/bot-names.js`): who is credited with what, when the scores stop,
  the bots' names. The world keeps all of it, so a worker or a replay
  keeps the same numbers (`worldState().scores`).

The original's behaviour follows its interface Lua (`ifs_teamstats`,
`ifs_personalstats`, `ifs_awardstats`, `ifs_mp_lobby`, `ifelem_listmanager`)
and was measured on captures of the 2004 game: the end tables of a Geonosis
and a Hoth Instant Action battle, a Naboo LAN game's end table and player
list, and the stat popup under the death camera (the captures are not in
this repository). The assets are in
[scoreboard-assets.md](scoreboard-assets.md); the rules as prose in
[formats.md](formats.md#scores-and-the-end-of-match-screens).

## What the original has, and what Tab does here

The PC game has three things that show scores:

- **The player list** (`ifs_mp_lobby`, "HOST LOBBY" or "CLIENT LOBBY"):
  Tab in **network games only**, and the pause menu's PLAYER LIST there.
  Offline the original drops the request: in Instant Action Tab shows
  nothing, and the pause menu has no PLAYER LIST. It is one list of the
  human players, no bots:
  - Columns NAME, TEAM (the team's name in its colour), PING and KILLS
    (`ifs.MPLobby.name_header`, `team_header`, `ping_header`,
    `ifs.Stats.kills`). CONNECTION (`qos_header`, the `ping_icon` bars)
    replaces PING and KILLS only on Xbox Live.
  - The player's own name is in his team's colour, the others' white.
  - The rows are the network session's player slots, not the Characters,
    sorted by team, then kills (most first), then name.
  - Tab opens the pause menu with "skip to player list" set; the pause menu
    pushes the list at once, and unpauses when the list closes.
- **The stat popup**: the player's own three numbers at the left edge for
  3 s whenever one changes.
- **The end-of-match table** (`ifs_teamstats`): every Character of both
  teams, side by side, after the battle.

Here **Tab shows the end table's layout during play** instead, offline and
online alike, bots and all, with the numbers live: every player's and
every AI unit's kills, deaths and command post points. That is the port's
own: the table is the original's, and so are the rules for holding and
tapping Tab, but the original never shows this table before the battle
ends. In a network game it also shows what the original's list has and the
table lacks: each player's **PING**, one column before KILLS; and a
player's DETAILS end with his **CONNECTION**.
A battle with the AI off (a duel server's `ai "off"`, or BATTLE AI: OFF)
has no bots, so its table lists the players alone: nobody fields the
mission's armies, and the roster drops them
(`src/runtime/multiplayer-roster.js`).

## Files

| File | What it does |
| --- | --- |
| `src/scoreboard/index.js` | `createScoreboard()`: Tab's table, the end table when the battle is over, DONE and the idle quit, the sounds, the mouse |
| `src/scoreboard/model.js` | Pure: the rows, their order, the selection and scroll, the page (table, DETAILS, AWARDS), the idle clock |
| `src/scoreboard/input.js` | Keys and mouse to the model; `TabHold`, the hold-or-tap rule; `bindDom()` |
| `src/scoreboard/port.js` | The world's side: rows (with a network game's pings), team titles, DETAILS' and AWARDS' contents, the battle's end, as plain values |
| `src/scoreboard/classic/view.js` | The Classic board as a layer of the battle HUD: the table and the logos |
| `src/scoreboard/classic/pages.js` | DETAILS and AWARDS |
| `src/scoreboard/classic/layout.js` | Pure: where everything sits for an interface height |
| `src/scoreboard/page/view.js` | The page view: the board in the front end's look, as elements over the battle (see [The page view](#the-page-view)) |
| `src/scoreboard/debug.js` | The board on demand over the running game |
| `src/battle.js` | The scores and when they stop: `credit()`, `point()`, `postDestroyed()`, `headshot()`, `vehicleDestroyed()`, `gameOver`, `finished`, `nameBots()` |
| `src/battle-stats.js` | Each Character's record for DETAILS and AWARDS, and the reports |
| `src/bot-names.js` | Bot names, each team from its own generator |
| `src/runtime/network-entity-summary.js`, `src/runtime/snapshot-host.js` | The scores and records on their way to the page, a world worker's or a server's (see [The page's numbers](#the-pages-numbers)) |
| `src/hud.js` | The stat popup (`drawStats`), the dead player's HUD, `watch()`, a layer's `teams: false` |
| `src/hud-units.js` | The message feed names the player by his Character's name, "Player 1" |
| `src/audio/game.js` | The `busFade` event; `interface` plays a sound by its own name too |

## Tab

- **Holding** Tab half a second or more shows the table until Tab is let
  go.
- **A tap** (let go sooner) pins it up. It closes when Tab is next held and
  let go.
- A click on BACK closes it. **Esc** puts the pause menu over it
  ([menus.md](menus.md)); closing that comes back to the table.

That is the original's own check (`ScriptCB_CheckPlayerListDone`, every
frame with whether Tab is held, measured from the opening):

- In the first half second, a frame without Tab marks a tap.
- After it, a held list closes on the first frame Tab is up.
- A tapped one turns into a held one as soon as Tab is down again.

**When Tab works.** Not while:

- the spawn screen is up (Tab picks posts there);
- the pause menu is up;
- the chat entry is open (T or Y; its own Tab does nothing, and the entry
  keeps the keys), nor does chat open while the board is up;
- the game is over (2 s after a team has lost).

The game over also takes it down. Alive, dead, on foot or in a vehicle:
all fine.

**While it is up:**

- The HUD's own displays hide: weapons, health, reticle, reinforcements,
  map, messages, the popup. That is the original's list.
- The soldier takes no input, and the world keeps running.
- The game's own sound fades out over a second, and back in over a second
  when the list closes. This is the `ingame` bus; the menu sounds are on
  `shellfx`, which isn't under it.
- The numbers are live.

## The battle's end

As the original's mission loop runs it:

1. **The win:** a team is out of reinforcements. The port's VICTORY or
   DEFEAT banner shows (the port's stand-in for the original's end display,
   which comes at the game over).
2. **The game over,** 2 s later. The scores stop counting, Tab's table
   closes, the spawn screen closes, and the music changes.
3. **The end table,** 7 s after the game over: the original's end-sequence
   timer, offline. The banner gives way to it and the game's sound fades
   out. The original can be quicker, 5 s after the game over, when its
   victory music has already finished. The port doesn't watch the music.
4. **Leaving:** DONE, or 30 s with no input (a frame takes at most half
   a second off that clock), leaves the table, from DETAILS and AWARDS too.
   (The original's Esc did too; here Esc on the table opens the pause menu
   over it, whose QUIT goes to the front end, and the idle clock waits
   while it is up.) As in the original, that **goes on to the next map** of the
   Instant Action playlist, which wraps round for ever (`nextUrl()` in
   `src/launch.js`; [shell.md](shell.md#after-a-battle)). A battle opened
   without a playlist starts again: the page reloads with the same URL.

## The table

**Who is listed:**

- Every Character of teams 1 and 2, the human and the bots, whether or not
  they have a body. Locals aren't listed.
- Team 1 is on the left and team 2 on the right, whichever the player is
  on.
- The shorter list is padded with blanks, so both scroll together.

**Names:**

- The player is "Player 1" (the original's fallback when the profile has
  no name), here and in the message feed ("GAMMA 935 KILLED PLAYER 1"). It
  is made in one place (`humanName()`, `src/bot-names.js`); his Character
  carries it, and both read it from there.
- A bot is named once, when its team's Characters are made (see
  [Bot names](#bot-names)).
- A bot that takes a hero class takes the hero's name for the rest of the
  battle.
- Names are shown in capitals and cut by whole characters to the name
  column's width.

**Columns:**

- KILLS is kills − 2 × team kills: a suicide counts as a team kill, so a
  lone suicide shows −1.
- DEATHS counts every death.
- CP POINTS counts command post points.
- PING, Tab's table in a network game only: the server's last round trip
  to each player in milliseconds. Bots have none, nor has a player before
  his first measure. It doesn't sort.

**Order:** kills, most first; then deaths, fewest first; then CP points,
most first; then the name by UTF-16 code unit **from Z to A**. That last
one is a quirk of the original. The captured end tables come out in exactly
this order.

A click on a column's icon sorts by it:

| Icon | Order |
| --- | --- |
| KILLS | kills, deaths, CP points, name |
| DEATHS | deaths, kills, CP points, name |
| CP POINTS | CP points, kills, deaths, name |

- The same icon again reverses its own key only. A third click puts it back.
- The order lasts for the session, Tab's table and the end table alike.
- The original's sort by name has no icon on PC, so it can't be reached.

**Colours:** bots grey (128, 128, 128), other humans yellow (223, 223, 32),
the player white.

**The selection:**

- It opens on the player's row, on his team's side.
- The lists are scrolled so five rows show above his: the Lua's
  FirstShownIdx = max(row − 4, 1), 1-based, with his row 0-based. It is
  then held to the last page.
- The row under the pointer gets a light-blue frame.

**Input:**

| Action | Keys and mouse |
| --- | --- |
| Up / down a row | Up / Down |
| A page | Page Up / Page Down: to the edge of the rows in view first, then a whole page |
| The other team's list, same row | Left / Right |
| Select a row | A click |
| DETAILS | A second click on the same row within 0.4 s; a click on DETAILS |
| Sort | A click on a column's icon |
| Scroll a row | A click on an arrow |
| Click at the pointer | Enter |
| AWARDS, DONE | A click (end table only) |
| Leave | DONE on the end table; BACK or Tab on Tab's table. Esc: the pause menu over the table |

**The mouse:**

- While the board is up, the game's controls are off and the page's cursor
  hides over the canvas, where the board draws `mousecursor`.
- A captured mouse stays captured and moves that pointer by how far it
  moves. A free mouse moves it to where it is.
- The pointer starts in the middle of the screen: the original warps it
  there.

## Drawing

The Classic view is a layer of the battle HUD (`hud.setLayer()`), in the
HUD's interface space: 1024 units across, H down (768 at 4:3, 624 at
16:9). The layout is the Lua's, inside the safe area (90% of the screen,
from 5% in).

| Piece | 4:3 | 16:9 | What |
| --- | --- | --- | --- |
| Dimming | the whole screen | | `opaque_black`, black at alpha 119, under everything |
| Window | centre (512, 370.2), 921.6 × 566.8 | (512, 300.8), 921.6 × 460.5 | `border_3_pieces`, nine-sliced, 16-unit corners |
| Titles | text from (66.2, 121.3) and (527.0, 121.3) | y 98.6 | "REPUBLIC : 147", the team's reinforcements left; `gamefont_medium`, white |
| Column icons | 30 × 30 from x 358.8 / 414.1 / 469.4 and 819.6 / 874.9 / 930.2, y 123.6 | y 100.5 | `stats_kills`, `stats_deaths`, `stats_cpp` |
| Arrows | centres (281.6, 165.6) up, (281.6, 637.4) down; 742.4 on the right | y 146.7 / 512.3 | `small_arrow`, 30 × 30, turned |
| Rows | 16 a side, 27 apart, the first's centre at y 202.2 | 13, from 166.8 | |
| Names | from x 68.2 / 529.0, 280 wide (215.4 with PING) | | `gamefont_medium` at 80% width, the line's top 10 above the row's centre |
| Numbers | right-aligned at x 382.4 / 439.7 / 497.0 and 843.2 / 900.5 / 957.8 | | the same |
| PING | right-aligned at x 325.1 / 785.9, one column before KILLS; its label centred on the icons | | the same; Tab's table in a network game |
| Selection | the row's 440.8 wide, 37 tall, 4 below its centre, centred on its list | | `listbox_cursor`, nine-sliced |
| Logos | centres (281.6, H/2) and (742.4, H/2) | | the command post holograms, size W / 800 |
| DETAILS | centred at (512, 662.9) on its tab | y 539.3 | `gamefont_small`; `backbutton_left`, `_mid`, `_right` |
| DONE | from x 71.2, centred at y 714.6 | y 577.8 | `gamefont_medium`; BACK in Tab's table |
| AWARDS | ending at x 957.8 − 5, centred at y 714.6 | | `gamefont_medium`, end table only |

**Text:**

- **The 80% width.** The glyphs narrow, but the space between them
  doesn't: each glyph advances round(0.8 × advance) + 1, as the captures
  measure. The text keeps its full height.
- **The intro.** Each row's texts play the interface's flashy intro (the
  white block sweeping across, the text growing from its middle, a
  blink), each after a random 0 to 0.15 s, whenever the table shows.

**Icons, arrows, buttons and frames:**

- The column icons and the arrows fade in over 0.4 s whenever the table
  shows.
- Hovered, the icons and arrows blink: alpha 0.4 to 1 over 0.4 s, back
  over 0.2 s.
- The arrows show only while there is more to scroll to that way.
- The Lua gives the arrows alpha 180, but its hover update sets their alpha
  every frame, to 1 when not hovered. The captures show them at full
  strength, as drawn here.
- Hovered buttons pulse between 0.3 and 1, twice a second.
- The selection frame's strength runs from 1 to 0.5 and back, once a
  second, but only on the right-hand list. The Lua fills the right list
  last, which leaves the pulse on it, or on nothing while the selection is
  on the left. The captures show the right-hand frame at different
  strengths, and Hoth's, with the player on the left, at full strength.

**The logos:**

- They are the command post holograms (`com_icon_*`) as 3D models, lit,
  at alpha 0.2.
- Their colour is the relation colour as the player's team sees the team:
  green (32, 223, 32) for his own, red (223, 32, 32) for the other.
- Whenever the table shows, they grow from half size and fade in from 0
  over 0.4 s, spinning at 30°/s. After that the left one spins at 9°/s and
  the right one at 7.5°/s.
- Coming back from DETAILS, that side's logo flies back from the window's
  corner, at 600°/s.

**Measured from the captures:**

- The labels' widths. The original measures a label wider than its glyphs
  run: AWARDS ends 5 units short of its place, and DETAILS is centred
  4 units to the left. That is what the captures show.
- DETAILS' tab runs from 22 units before the text to 20 after it, ends
  square. That follows `bgoffsetx` −22 and `bg_tail` 20, and the capture
  matches it to a unit.

**Side by side.** The captured tables were rebuilt on the port
(`tests/scoreboard-capture-fixture.js`) and measured against the
unsqueezed captures. Titles, icons, every row's name and numbers, the
selection frame, DETAILS, DONE and AWARDS all land within one unit. The
selection frame lands exactly.

## The page view

The battle shows the board in the front end's page look
(`src/menus/themes/page`, [menus.md](menus.md)) unless its URL has
`menus=classic`: `src/game.js` names the view `"page"`, and `VIEWS` in
`src/scoreboard/index.js` has it beside Classic, which stays the default
when no view is named. It is the same board behind the same model and
input, drawn as elements instead of on the HUD:

- **Over the world:** a fixed layer at z-index 10, under the pause menu's
  page (20). The running world shows through `--bfe-overlay` and a 4 px
  blur. Its HUD layer only hides the HUD (`teams: false`); nothing is drawn
  on the canvas.
- **The looks:** eternal, bf1 and bf2, through the look's custom properties
  and the theme's title, tabs and small buttons. The table follows the
  server browser's: an accent header bar (bf1's angled, bf2's grey metal
  with a red line), framed selection, lit hover.
- **The page:** the title STATS (`ifs.stats.statstitle`), tabs for the
  table, DETAILS and AWARDS (the end table's only), the page, and DONE (BACK
  during play and on DETAILS and AWARDS), DETAILS and AWARDS along the
  bottom, where the Classic board has them. The content and the bottom row
  share `--bfe-page-frame`.
- **The table:** both teams side by side, each with its name in its
  relation colour (green for the player's, red for the other: the Classic
  logos' colours) and its reinforcements, over NAME, PING (Tab's table in
  a network game), KILLS, DEATHS and CP POINTS. The sorted column has an
  arrow, down while the most come first. Bots are dim, other players in the
  look's highlight, the player's own row lit. Only names are cut short, with
  an ellipsis.
- **Scrolling:** both lists scroll together, natively (the wheel, the thin
  scroll bar). The model's first row follows them, and its shown is how
  many rows show whole, so the keys keep the selection in view as before.
  A list with more below fades out at its foot.
- **DETAILS:** PERSONAL STATS, the row's name, his team and his KILLS,
  DEATHS and CP POINTS, then the record's rows, labels before the middle
  and values after it, as the Classic page has them.
- **AWARDS:** PLAYER AWARDS, each winner after a tag of his team, or a dash.
- **The pause menu** over the board hides it (`hidePointer`); it comes
  back when the menu closes.

**Input.** The keys are the input's own. The mouse goes through the same
input as the Classic board's: `hit(x, y)` reads the element under an
interface point.

- A free mouse moves the input's pointer as it moves, and a press is the
  input's click there. So hover, a row's select and double click, a
  header's sort and the buttons act as on the Classic board.
- A captured mouse stays captured and moves the input's pointer by how far
  it moves (`bindDom`); the view draws it as an arrow, and the wheel
  scrolls the list under it.
- The tabs are their pages' buttons: the table's is BACK. DETAILS and
  AWARDS go straight from one to the other through the table, with one
  sound (`act()` in `index.js`).
- No element takes the keyboard focus.

## DETAILS

The record of the selected row's Character (`ifs_personalstats`). It
opens from DETAILS or a double click, from the end table or from Tab's.

- **Title:** "PERSONAL STATS" and the row's name, centred at the top
  (`gamefont_medium`). The title fades in over 0.4 s.
- **Rows:** three blank rows under the title, then nine rows in
  `gamefont_small`, 31 units apart. Each label is right-aligned ending
  15 units before the middle, and its value left-aligned from 15 after.

  | Row | Shows |
  | --- | --- |
  | HEAD SHOTS | his head hits |
  | FAVORITE CLASS | the class he spent most time as, and the time: "CLONE TROOPER 3:07" |
  | FAVORITE WEAPON | likewise, the weapon in his hand or his seat's |
  | FAVORITE VEHICLE | likewise; empty if never in one |
  | LONGEST LIVING STREAK | " m:ss", leading space and all |
  | NARROW ESCAPES | times under 10% health and back to full |
  | VEHICLES DESTROYED | vehicles he destroyed |
  | NEMESIS | who killed him most: "UNIT 538 (2)", or empty |
  | BAIT | whom he killed most, or empty |
  | CONNECTION | a network game's players only (the port's own row): the path the server reaches him by, "DIRECT IPV4", "DIRECT IPV6", "RELAY IPV4" or "RELAY IPV6", the status line's words |

- **During play** the record is the world's of up to 5 s before, in whole
  seconds, and the player's own of a tenth of a second before (see
  [The page's numbers](#the-pages-numbers)).
- **The logo:** his team's logo flies from its place on the table to the
  window's top corner on that side, at half size and alpha 0.4, over
  0.4 s, spinning at 600°/s, then at 9°/s.
- **Leaving:** BACK or Esc goes back to the table, selection kept.
- **Unanswered keys:** Up, Down, Left and Right only keep the idle clock
  from running out.

Checked against the original (the Hoth capture, PLAYER 1's DETAILS): the
title, the name line, every label's extent, the values' left edge and BACK
all land within one unit. The logo lands in the window's corner on its side,
as in the original.

## AWARDS

The battle's eight awards (`ifs_awardstats`), from the end table.

- **Title:** "PLAYER AWARDS". Two blank rows under it, then eight rows in
  `gamefont_medium`, 34 units apart. Labels end 25 units before the
  middle; values start 25 after it.
- **Values:** "NAME (n)", or empty when nobody won.
- **Icons:** each winner's team logo flies from his team's side of the
  table to its row, between label and value. It goes from size W / 800
  down to 0.2 and from alpha 0.2 to 0.3 over 0.4 s, spinning at ±600°/s.
  After the first lands, each spins at a random −13.5 to 13.5°/s. Locals
  have no logo.

| Award | Goes to the most |
| --- | --- |
| TANK BUSTER | vehicles destroyed |
| KILLING SPREE | kills inside 5 s |
| CAMPER | time within 10 m of one spot, shown "(m:ss)" |
| SURVIVALIST | enemy kills while under about 25% health |
| TRAITOR | team kills, suicides included |
| PUBLIC ENEMY | enemy kills in one life |
| BANTHA FODDER | deaths |
| DEAD EYE | head shots |

**The winner:**

- Only more than the best so far wins, and the best starts at 0. So
  nothing wins with 0, and ties go to whoever comes first.
- The search goes through slot 0 (team 1 and locals) before slot 1
  (team 2), each in roster order.

Checked against the original too (the Hoth capture: CAMPER and BANTHA
FODDER won, the rest empty): the title, every label, the values and BACK
land within one unit, and the icons sit between label and value, in their
team's relation colour.

## The stat popup

The original's stat popup, in `hud.js` `drawStats()`:

- **Where:** from the safe area's left middle, three rows 32 apart, for
  KILLS, DEATHS and CP POINTS. Each row is drawn at 0.75 from 36 units in:
  its icon 24 units square from 9 to 33 units in, its number in
  `gamefont_small` ending 63 units in, centred on the row.
- **At rest** both icon and number are white at alpha 64, faint.
- **A change** turns that row's icon and number yellow if it rose, red if
  it fell. The colour holds for half a second, then blends back over the
  next half.
- **Showing:** any change shows the popup for 3 s, fading over the last
  second. The match start is quiet.
- **Whose:** the player's Character, alive or dead, so a death shows it
  with DEATHS flashing. It shows under the death camera: it is the ordinary
  3 s popup, not held up while he is dead (the captured death-camera frames
  show it flash, rest faint, and go about 3 s after the death).
- **Gone** under a layer (the board, the spawn screen) and once the game is
  over.

**Dead,** the player has no body, and the original's HUD shows only what
isn't the body's: the reinforcements, the messages and the popup. The
minimap, the weapon and health panels, the reticle, the capture timer and
the scope go at the death and come back with the next body (`hud.js`, given
`dead` with the player each tick). The captured frames show exactly that set.

## Scoring

The world credits as the original does. Nothing counts once the game is
over.

| Event | Credit |
| --- | --- |
| An enemy's death | His killer (the Character that last hurt him): a kill |
| A friend's death, his own included | His killer: a kill and a team kill, so KILLS −1 |
| A neutral's death | Nothing |
| Any death | The dead: a death, killer or not |
| A neutral post captured | A CP point for each living Character in its capture region, the first 64 |
| A command post destroyed | A CP point for its last damager, unless he is its owner's friend |

`postDestroyed()` is called for the destructible posts
(`ObjectivePost` in `src/props.js`, [objectives.md](objectives.md)) and for
the command vehicles' posts (`src/mobile-posts.js`).

**The record** (`src/battle-stats.js`) keeps, per Character and per team
slot, what DETAILS and AWARDS need:

- **On each kill:**
  - kills per victim;
  - team kills;
  - enemy kills in this life;
  - enemy kills while his own health was under 64/255;
  - kills inside 5 s.
- **On each death:** the death; this life's kills and clock start again.
- **On each head hit:** a head shot, for whoever had hurt the soldier
  last *before* that hit. That is the original's order: the record is
  updated after the callback that counts the head shot.
- **On each vehicle destroyed:** a point for whoever hurt it last,
  whatever his team. Turrets and droidekas don't count.
- **Every world tick,** for every bot (waiting to respawn included) and
  for the player from his spawn until his spawn screen opens:
  - time in his class, and this life's length;
  - with a body: time with his weapon and in his vehicle, time within
    10 m of one spot, and his health. Under 10% and then back to full is a
    narrow escape.
  - The time tables keep 5 classes, 10 weapons and 5 vehicles; a new one
    replaces the one with the least time.

**KILLING SPREE's quirk:**

- The original counts kills inside 5 s in a ring of the last hundred
  kills, shared by everyone.
- Its count tests the slot stored at the ring's next free entry instead
  of each entry's. Until the ring has wrapped, that is 0, so only team 1's
  (slot 0's) sprees count.
- The port keeps this, so the award goes where the original's would.

## The page's numbers

The battle runs in a world worker offline and on a server online; the page
draws it from the snapshots either way (`src/runtime/snapshot-host.js`),
so the board reads Characters the page makes from them:

- **The roster** (world summary schema 4, `network-entity-summary.js`):
  every Character's team, whether a player holds it, kills, team kills,
  deaths, command post points, the server's round trip to that player and
  its path, and the name. Each chunk of twelve refreshes every second.
- **Everyone's records** (schema 5, ids from 80000): what DETAILS and
  AWARDS read (`battle-stats.js` `packRecord`). During play each chunk
  refreshes every 5 s in whole seconds, since their times change every
  second; once the scores stop they come exact, for the end table, within
  5 s, before it opens.
- **The player's own record** (id 79999) rides his own datagram ten times
  a second, in whole seconds. Until the scores stop his slot takes only
  that one, so his DETAILS never steps back to everyone's older copy.

The records' format is the end table's of `bfe-sim-15`; only how often
they come changed, so the simulation revision stays.

## Bot names

Each team draws its bots' names from a generator of its own, seeded with
its side type (Rebels 1, Empire 2, Republic 3, CIS 4) just before its
Characters are made. The human doesn't draw. So a side's bots have the
same names every battle.

| Side | Name | Examples |
| --- | --- | --- |
| Republic | A Greek letter and three digits | DELTA 855, THETA 269, CHI 987 |
| CIS | "UNIT" and three digits | UNIT 538, UNIT 182, UNIT 790 |
| Rebels | A first and a last name | THRAK FARELLE, RAYCE FARELLE |
| Empire | Two letters and three digits | DX 178, KL 987 |

- Every name on the Geonosis capture is in these sequences, and so is
  every Empire and Rebel name on the Hoth capture.
- Locals are all called the level's local name ("GEONOSIAN").
- The lists come from the map's `ai-mission.json` (`names`).
- The Classic AI's shared generator draws no bot names, so the draws after
  them at the match start (the quota padding, each Character's first
  objective, class and post) don't depend on them; `tests/ai-population.test.js`
  pins those sequences.

## Sounds

Through the `interface` sound event, by the sounds' own names:

- `shell_select_change`: sorting, moving the selection, scrolling.
- `shell_menu_enter`: DETAILS and AWARDS.
- `shell_menu_exit`: leaving them, closing Tab's table, and DONE or the
  idle quit.
  - The original's idle quit from AWARDS plays `shell_menu_enter`; that
    quirk is kept.
- Opening Tab's table is silent.
- The `busFade` event takes the `ingame` bus out over a second when the
  board opens, and back in when Tab's table closes.

## Not the original's

- **Tab's table during play,** as above, offline and online. Its BACK is
  the network list's, at DONE's place. Online it lists the bots too, and
  their deaths, where the original's network list has only the players'
  names, teams, pings and kills.
- **PING in the table** and **CONNECTION in DETAILS** online, from the
  original's network list (whose PC version shows no CONNECTION).
- **No PLAYER LIST in the pause menu** of a network game yet; Tab is the
  way in.
- **DETAILS from Tab's table.** The original reaches DETAILS only from the
  end table.
- **The end table's world:** the original's end camera isn't modelled. The
  table dims whatever the player's view shows.
- **The end of the end:** DONE and the timeout load the playlist's next
  map, as the original does, by navigating to its URL.
- **The end display:** the VICTORY / DEFEAT banner is the port's, shown at
  the win rather than at the game over.
- **The page view** ([above](#the-page-view)), the battle's board unless
  the URL asks for the original's menus: the front end's look, the teams'
  names in their colours in place of the logos, tabs between the pages,
  and DETAILS' KILLS, DEATHS and CP POINTS.
- **No gamepad, and no ifs_mp_lobby.**
- **The selection keeps its index** when a sort or a refresh moves the
  rows, as the original's lists do.
- **Edge-triggered Tab.** The board opens on Tab's press, not on any frame
  it is held. So Esc while Tab is held doesn't reopen it at once.
- **Head shots** count only hits that do damage. With friendly fire off,
  a friend's hit to the head credits nothing.
- **The AWARDS icons' start for locals** isn't modelled: locals have no
  logo, so there is nothing to fly.

## Checks

```sh
node --test tests/scoreboard-model.test.js tests/scoreboard-input.test.js tests/scoreboard-index.test.js \
  tests/scoreboard-port.test.js tests/scoreboard-scoring.test.js tests/scoreboard-page-view.test.js tests/battle-stats.test.js \
  tests/bot-names.test.js tests/snapshot-host-records.test.js
PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/scoreboard-browser.mjs
BFE_SERVER_REPO=<bfeserver checkout> PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/scoreboard-network-browser.mjs
```

**The node tests cover:**

- the order of every mode, reverse, and Z to A names;
- both captured end tables, from shuffled rows;
- the opening scroll, Up and Down, pages, Left and Right, the arrows;
- the idle quit;
- the hold-or-tap rule, frame by frame;
- clicks and the double click;
- the board's layer, controls, sounds and fades, and the battle's end;
- DETAILS and AWARDS straight from one to the other; the page view's name,
  its HUD layer, its sort arrows and its tabs;
- the credits, suicides, CP points, and the game over's freeze;
- the record: the spree ring's quirk, the tallies, DETAILS' formats, the
  award rules;
- the bot names, against the retail sequence when the map is extracted;
- a network game's rows: bots and players alike, each player's ping, his
  CONNECTION in DETAILS;
- everyone's records reaching a client during play every 5 s in whole
  seconds, exact after the game over, and the player's own slot keeping
  his own record (nab2, when extracted).

**The browser test**, on geo1 with the Classic AI and the Classic board
(`menus=classic`, as in the network browser test):

- Tab held and tapped, with real key events;
- both rosters listed by name, bots included;
- the board on the HUD's layer, with the controls off;
- Esc: the pause menu over it, and back;
- with the spawn screen up, Tab picks posts and no table shows;
- the battle's end run by the world: the banner, the game over 2 s later,
  the end table 7 s after, the frozen scores;
- DETAILS from Tab's table: a bot's record during play;
- DETAILS, AWARDS and BACK;
- DONE, and the 30 s idle quit;
- the popup flashing after a kill;
- dead: no minimap, weapons or health, "… KILLED PLAYER 1", DEATHS
  flashing.

It writes screenshots at 1024 × 768 and 1280 × 720 into the folder
`SCOREBOARD_SHOTS` names, including the three captures' tables rebuilt
(and Hoth's DETAILS and AWARDS) for side-by-sides.
`SCOREBOARD_PARTS=play,captures,popup,death,spawn` picks parts; a full run
takes about an hour under SwiftShader.

**The network browser test** starts a bfeserver (nab2, 32 a side) and joins
two rendered clients over the native UDP transport, one on each team:

- Tab held: every Character of both teams, the bots by name with their
  kills and deaths and no ping, both players with their pings; let go, it
  closes;
- a death on one client shows on the other's table;
- DETAILS from Tab's table: a bot's record during play, and the other
  player's CONNECTION;
- Tab in the chat entry keeps the focus there and opens nothing; T over the
  table opens no chat.

It writes both clients' tables and the two DETAILS pages at 1280 × 720 into
the folder `SCOREBOARD_SHOTS` names.

**From the console, over the running game:**

```js
const { scoreboardDebug } = await import("/src/scoreboard/debug.js");
scoreboardDebug(__bf, "end"); // or "peek"
```
