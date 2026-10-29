# The player's spawn: the world side of the spawn screen

The original's spawn screen (unit and class select, team switch, spawn post)
has two sides here:

- **The world side** (this document): when the player may spawn, what he
  spawns as and where, his team, and the bodies he plays. It is headless, so
  the same code runs wherever the world runs: the world worker, a server,
  replays.
- **The screen** (`src/spawn-screen/`, `docs/spawn-screen.md`): what is drawn
  and how input picks a choice. It only reads the world through the spawn
  port and sends choices as world commands.

The original's behaviour is in [formats.md](formats.md) ("Spawning and the
spawn screen").

## Files

| File | What it does |
| --- | --- |
| `src/mission.js` | Pure: which of the map's battles the URL plays; its mission is the one the policy and the screen read |
| `src/spawn-screen/policy.js` | Pure: from the URL, whether the screen runs and which side and class are played first |
| `src/human-spawn.js` | Headless: the player's phases, his queued choices and how they apply; the spawn-post rules; the team switch |
| `src/player-body.js` | One body per class the player takes, built on first use and kept |
| `src/units.js` | `buildBody()`, shared by the bots' factory and the player's bodies; `Unit.setBody()` |
| `src/game.js` | `adopt()`, `setHumanTeam()`, the commands' place in the tick, and `spawnPort` |
| `src/runtime/commands.js`, `transport.js`, `headless-ports.js`, `src/input.js` | `command.human`: building, validating and forwarding the choices |
| `src/mobile-posts.js` | Command vehicles (the AT-AT, the AT-TE) as posts while they live |

## When the screen runs

`spawnPolicy(query, mission)` decides, the same way in the page and in the
worker:

- If the URL has `side=`, `unit=` or `spawn=`, there is no screen. The world
  confirms that choice at load and after every death, as the port always
  did, and ignores the screen's team, spawn and cancel commands. The pause
  menu's RESPAWN (`suicide`) still works: a normal death, after which the
  URL's choice is confirmed again.
- Otherwise the screen runs: the player starts with no unit.
- `?spawnscreen=1` or `?spawnscreen=0` overrides either way.

The class played first:

- `unit=` names it.
- `side=` alone picks that side's first non-hero class in the mission.
- With neither, the screen's default is the first class of the team that
  fields the old default unit, `rep_inf_jet_trooper`. On Theed that is the
  clone trooper on the Republic; a mission without it uses team 1.
- A bypass without either (`spawn=` alone, or `spawnscreen=0`) keeps the old
  default unit itself.

The team is the mission team whose roster lists the class, or whose hero it
is, else team 1. Without `side=` the side is the class's own.

## Phases

`humanState().phase`:

| Phase | Meaning |
| --- | --- |
| `unspawned` | The match has started and he has never had a unit. The screen is open: the original's Character starts with no unit and a spent timer. |
| `alive` | He has a unit. |
| `dying` | Dead, with his 4 s respawn timer running. Nothing of the screen shows. |
| `awaiting` | Dead with the timer done: the screen is open. |

Who owns the timer:

- **Classic AI:** the spawn manager owns the timer and the spawn.
  - The screen opens when the human Character has no unit and its timer is at
    or below zero (`awaitingHuman`'s test, without its `ready` condition: the
    screen stays open while a choice waits for a post, as in the original).
  - The manager counts in float32, so after a death the screen opens on the
    241st tick.
- **AI off or stub:** `HumanSpawn` keeps its own 4 s timer and opens on the
  240th tick.

**Unspawned:**

- His Unit is not alive, `removed`, and not targetable, so nothing counts,
  targets, captures with, or collides with him.
- His body is hidden and parked at his team's first post. The world's camera
  (`simulationCamera`, which Classic scheduling reads) follows that parked
  point, so it stays deterministic.
- The drawn camera follows it too, unless the screen sets a camera shot.

**Death:** the body lies where it fell for 4 s (`CORPSE_LINGER`), then leaves
the world as a bot's does: hidden, removed, no longer stopping rounds. A
death region kills him like anything else (in the original it deals an empty
damage record and deletes nothing), so his body lies where it falls for the
same 4 s.

**The camera while dead:** the original's death camera (formats.md, "Death
camera"; `DeathCamera` in `src/camera.js`) takes over from whatever view he
had, looks down on the body, and from 1.5 s turns to the killer.

- It is the world's camera (`simulationCamera`), as the original's camera is
  what its AI scheduling reads. So it steps in `step()` from world state
  alone; the worker and replays get the same one. It also names the killer
  in plain numbers (`DeathCamera.blame`), which the page's death shot reads.
- The drawn camera is the page's own (`src/death-shot.js`, the Death Camera
  option: Enhanced or Classic), from the body and the killer as the page
  draws them, until the screen's camera shot covers it (`setViewShot`) or
  he spawns (`placePlayer` stops it). Online, where the page takes no
  damage itself, the server's damage and death records name the killer.
- Once the body has left the world, the body point stays where it was.
- A droideka has none: its chase camera stays on the wreck, as in the
  original.
- Dead, his body is drawn even in first person, and the world's camera is
  never his eyes.

## Choices are world commands

`spawnPort.request()`, `suicide()` and `cancel()` don't touch the world.
They queue entries on the controls, which send them with the next command
as `command.human`:

```
{ kind: "team", team }               // 1..7; applies only while he has no unit
{ kind: "spawn", className, post }   // post: a post name, or null for the world's pick
{ kind: "suicide" }                  // the pause menu's RESPAWN; only while he has a unit
{ kind: "cancel" }                   // withdraw a spawn choice not yet carried out
```

- `humanEntry()` keeps only these values; anything malformed is dropped (the
  port) or rejected (the worker's mailbox).
- The queue holds at most one entry of each kind (`queueHumanCommand`): a
  team, spawn or cancel at its last place, a suicide at its first. A burst
  of choices over a stalled frame does what all of it would have, and never
  outgrows the mailbox's 16 entries a command.
- `CommandHistory` keeps them with the command.
- `InputMailbox` validates them and merges a late batch's entries in order.
- A cleared input (focus or pointer lock lost) doesn't drop them. A command
  stamped with a previous life's generation does, with the rest of its input.

They are applied in `step()`, inside `updateBattle()`, right after the
battle's update and the Classic spawn manager's own per-Character update.
That is the point where the port used to auto-confirm the player.
`HumanSpawn.update()` then:

1. applies the entries in order;
2. works out the phase;
3. if a spawn choice waits and he may spawn (`unspawned` or `awaiting`),
   realizes it.

How a choice is realized:

- The class must be in his team's roster and loaded; otherwise the choice is
  dropped with a warning.
- **Classic:** the choice goes to `onHumanReady({ className, post })`, and the
  manager places him on its next update at a random point of the post's spawn
  path.
  - If the post no longer serves, the manager moves it to the eligible post
    nearest it.
  - With no eligible post he waits, ready.
  - A new choice made while he waits replaces the old (the original's BACK,
    then SPAWN), and `cancel` withdraws it: the Character's ready flag is
    cleared, as the original's uncommit does.
- **AI off or stub:** he is placed at once, at a random point of the post's
  spawn path, its ally path for an ally's post, else a class spawn point,
  else the post itself (`spawnPointAt`).
  - The random draw is the gameplay stream's.
  - An ineligible post gives way as in the original.
  - With nothing eligible at all, his team's first post with a spawn path is
    used, as the port did before the screen.

Once the battle is over nobody spawns, as the original closes its screen at
game over: team and spawn choices are ignored, a choice waiting (here, or
held by the Classic spawn manager) is dropped, `awaitingSpawn` isn't sent,
and a dead player's phase stays `dying`. `humanState().over` is true then,
for the screen to close. Runs without a screen keep their automatic
respawn, as before.

Events are queued during the tick and sent after it (`flush()`). A listener
that throws is reported and doesn't stop the battle.

## Bodies

`PlayerBodies` (src/player-body.js) keeps one body per class. Each body is:

- a bot's body (`buildBody`): the model re-skinned by `OverrideTexture`, a
  `Soldier` or a `Droideka`, and its animator;
- what only the player's body has: `PlayerCombat` aimed from the world's
  camera with the HUD, the first-person arms (the droideka's with its own
  clip set and offset), a droideka's shield bubble and dust, and a jet
  trooper's jetpack effects.

Building and loading:

- The class played first is built from what the game fetched at load.
- Any other class is built from the bots' factory's copy of its model the
  first time he takes it.
- With the screen, every non-hero class of teams 1 and 2 is loaded at map
  start, in every AI mode: its model, a droideka's clips, its first-person
  rig, its HUD icons, and the textures its body, arms, weapons, rounds and
  pieces show (with the AI off no bot has loaded them). A class that fails
  to load is left out of the roster. When the URL names his unit, which can
  never change, only that class is loaded (and the bots' own, when the AI
  runs).
- The rosters are fixed once the map has loaded, and kept: the screen asks
  for the state every frame.

`adopt(bundle)` gives him another body, only while he has no unit. It is
called from the spawn itself (`spawnUnit` with the Classic AI, `spawnHuman`
without), so the body always matches the class spawned. His Unit stays the
same object, keeping its id, generation, team and Character.

What `adopt()` moves to the new body:

- the old body is hidden; its bubble, dust and jet flames stop; its arms
  leave the first-person scene;
- the new body takes the old one's place and state: dead, at the same spot;
- every game.js binding (soldier, actor, animator, combat, presentation,
  clips, def, walker, arms, bubble, jetpack, ribcage, health type, the
  side's items);
- `Unit.setBody()` (class, kind, hero, root scale, hit shapes);
- both soldier cameras' sections and fields of view (`SoldierCamera.setDef`);
- the vehicle driver and the turret gunner;
- the mouse look preview and water contact;
- the player's sounds (`audio.retarget`);
- `wireBody()`: the combat's owner, team, items, droids, lightning, turrets,
  vehicles, effects, arms and death and respawn callbacks, and a droideka's
  shield and blast hooks;
- the perspective (layers);
- `window.__bf`'s body fields, which are getters.

Recon droids already out keep flying after he changes class; they know every
side's fire points.

## Switching team

`setHumanTeam(team)` applies only while he has no unit, and only between
teams 1 and 2. No ticket is spent: the death already cost one. There is no
balance rule; team capacity is 200, as offline.

`switchHumanTeam()` moves every place the team is kept, together:

- with the Classic AI, its roster first (`spawning.switchTeam`), which moves
  the Character and clamps his class and post. His place in his post's
  queue is given up under the old team (its own post or an ally's) and
  taken at the new team's post. If it refuses, nothing changes. The bots on
  his new side then use the player curves (`difficulty.playerTeam`), the
  living ones included (`refreshCurves`). Without it, the Character's team.
- game.js's `playerTeam` and `enemyTeam`, and every closure that reads them:
  friendly fire, the HUD's relations, items, kill messages, Victory or Defeat;
- `posts.playerTeam`: the holograms' and maps' relation colours;
- `battle.own` and `battle.foe`: the HUD's bars, Victory or Defeat;
- `live.playerTeam`: the repair and lock-on default;
- every body's `combat.team`: rounds, mines, droids;
- `playerUnit.team`;
- music and voice-over (`audio.retarget`).

Rosters become uneven: the human is one of his team's `SetUnitCount`. The
original doesn't rebalance either.

## The spawn port

The screen reads the world only through `game.spawnPort` (and
`window.__bf.spawnPort`):

| Call | Notes |
| --- | --- |
| `humanState()` | `{ phase, team, className, classIndex, post, canSwitchTeams, over }`. `className` is the class of his current or next life; after a team switch it is clamped into the new roster, as the Character's index is. `post` is the last post he spawned at, by name. |
| `teams()` | `{ 1: { side, logo, objectives }, 2: ... }`. `side` comes from map.json `spawnScreen`, else `SetTeamName`, else the first class's side. |
| `roster(team)` | Non-hero classes, in mission order, that loaded. `displayName` falls back to the class id. `weapons` are in slot order: `displayName`, else `label`, trimmed. |
| `posts(team)` | Every post. `eligible` uses the Classic spawn manager's relations when it runs, else the registry's. `vehicles` comes from map.json `spawnScreen.postVehicles`. |
| `choosePost(team, previous)` | A post name or null. Its random fallback uses `Math.random`: it runs on the screen's side, and its result goes into a command as a plain name. |
| `request({ team, className, post })`, `suicide()`, `cancel()` | Queued as `command.human`. |
| `previewBody(className)` | Built with `buildBody`, outside the world: its shadow isn't a world caster, and its motion proxy is released by `dispose()`. It holds its primary weapon in that weapon's bank, faces +z, and aims at (0, 10, 4000). Every 3 to 8 s it plays the stand idle emote once as a fidget; the extracted banks map the original's fidget names to that clip. A droideka cycles idle, the turns, fold, ball and unfold. Also returns `materials` and `walker`. |
| `cameraShots()` | map.json `spawnScreen.cameraShots`. |
| `setViewShot(shot)` | Sets the drawn camera's position and quaternion every frame, after the soldier camera and its shake, until it is cleared with null; the audio listener follows. `simulationCamera` is never touched. Changing the shot resets the temporal upscaler's history. |
| `commonModel(name)` | A fresh copy of a common hologram-pack model (`com_icon_*`, `shell_cube`). Each part has its own unlit, transparent, double-sided material, listed in `userData.materials`, and each mesh's texture scroll is in `userData.scroll`. Null headless. |
| `on(event, fn)` | `awaitingSpawn`, `spawned`, `teamChanged`, `playerDied`; returns an unsubscribe function. |

`window.__bf` also exposes `humanSpawn`, `bodies`, `policy`, `adopt`,
`setHumanTeam` and `playerTeam` for checks, and `worldState()`
(src/runtime/world-state.js) includes `humanState`.

## Command vehicles

A command vehicle is a post while it lives, as in the original
([formats.md](formats.md), "Spawning and the spawn screen"): Hoth's two
AT-ATs, Geonosis' two AT-TEs and Rhen Var Harbor's AT-AT or AT-TE. Every
class on the `commandwalker` or `commandhover` base qualifies (the
MTT and the hovernaut too, which no stock map places).

- **The posts:** `CommandPosts.attachVehicles()` makes one post per vehicle
  pad whose classes include a command vehicle, named by the pad's slot
  (`transport-<slot>`, the names the network registry already reserves,
  `src/runtime/entity-spawn-posts.js`). `CommandPosts.all()` lists them
  after the placed posts; `posts.posts` stays the placed ones, which the
  holograms, the capture, the maps' flags and the post summaries read.
- **Live:** a pad's post is active while its command vehicle lives (health
  above 0); a dying walker is no post. Its next vehicle makes it active
  again.
- **Owner:** the hull's team at spawn; whoever takes a seat makes it his
  team's, and it stays his when he gets out.
- **Where:** the hull's position now, for the nearest-post rules and the
  AI's destination.
- **Spawn points:** the class's (ai-mission.json `commandClasses`), turned
  and moved with the hull and pulled back toward its collision centre to the
  first terrain, building or prop between them
  (`commandSpawnPoint`). The AT-AT's six stand 8 m out to either side.
  `spawnPointAt` and the Classic spawn manager draw one, as for a placed
  post's class spawn points.
- **The battle:** its `ValueBleed` counts for its owner's bleed and holding
  it resets the no-post count (`src/battle.js`); its weights make it an
  objective (`src/ai/classic/goals.js`).
- **Lost:** when it dies the authority's `updateBattle` reports it once
  (`CommandPosts.mobileEvents()`): the AI's bots sent there draw again, its
  last attacker earns a post point unless he is the owner's friend, and
  "EMPIRE LOST THE AT-AT" is shown. A page showing a server's battle raises
  that message from the hulls it shows. A hull an enemy takes reports a
  change of owner to the AI.

Everywhere a spawn is chosen reads `CommandPosts.all()` with the landed
transports: the screen (`spawnPort.posts`, `choosePost`), the server's
spawns for every human (`entitySpawnPosts`, single player included), the
Classic spawn manager and `spawnHuman`/`readyHuman`. On a page showing a
server's battle the posts come from the hulls it shows, and whether they
serve from the server's eligibility.

## Checks

- `tests/spawn-policy.test.js`: the bypass rules.
- `tests/human-spawn.test.js`:
  - the phases and command rules, with AI off and with a real Classic spawn
    manager;
  - that every team holder moves (or none does);
  - the post rules.
- `tests/mobile-posts.test.js`: command vehicles as posts: which classes,
  live and dead hulls, the owner, the spawn points and their pull-back, the
  events, the bleed and the no-post count, a bot's spawn, the screen's
  choice and the objectives.
- `tests/human-commands.test.js`:
  - command validation, the mailbox's merge, the controls and the history;
  - `Unit.setBody`.
- `tests/human-spawn-browser.mjs`, on Theed with the AI off, through the
  production entity session (`checkSpawnLifecycle` in
  `tests/local-server-checks.js`):
  - the unspawned start at the spawn screen;
  - for every class of both teams: a death that opens the screen after its
    delay, a spawn whose team and class follow the world, the presentation
    body and first-person arms rewired, and a recon droid or dispenser
    reaching the presentation;
  - the session staying healthy after each class.

## Gaps

- **No `?switchteams=` option:** team switching is always allowed (Instant
  Action). The campaign modes' lock isn't modelled.
- **HUD while he has no unit:** the HUD still shows the last body's weapons
  and an empty health bar. Hiding them while the screen is open is the
  screen's job.
- **Fidget:** the fidget clip is approximate, as above.
