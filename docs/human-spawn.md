# The player's spawn: the world side of the spawn screen

The original's spawn screen (unit and class select, team switch, spawn post)
has two sides here:

- **The world side** (this document): when the player may spawn, what he
  spawns as and where, his team, and the bodies he plays. It is headless, so
  the same code runs wherever the world runs: the client's local world
  thread, a server, replays.
- **The screen** (`rust/bfe-battle-ui` (`spawn`), `docs/spawn-screen.md`): what is drawn
  and how input picks a choice. It only reads the world through the spawn
  port and sends choices as world commands.

The original's behaviour is in [formats.md](formats.md) ("Spawning and the
spawn screen").

## Files

| File | What it does |
| --- | --- |
| `rust/content` (`mission`) | Pure: which of the map's battles is played; its mission is the one the policy and the screen read |
| `rust/bfe-battle-ui/src/spawn/policy.rs` | Pure: whether the screen runs and which side and class are played first |
| `rust/sim/src/battle/human.rs`, `human_world.rs` | The player's phases, his queued choices and how they apply; the spawn-post rules; the team switch |
| `rust/sim` (`world::admit`) | One body per class the player takes, built on first use and kept |
| `rust/sim` (`units`) | Building a body, shared by the bots' factory and the player's bodies |
| `rust/sim` (`wire`), `rust/sim` (`input`) | The human command: building, validating and forwarding the choices |
| `rust/bfe-client` (`app/battle.rs`) | The spawn port over the world: what the screen reads and the commands it sends |
| `rust/sim` (`battle::mobile`) | Command vehicles (the AT-AT, the AT-TE) as posts while they live |

## When the screen runs

`spawn_policy` decides, the same way in the client and in the world:

- If a side, unit or spawn post is chosen in advance (`side`, `unit`,
  `spawn`: the scripted runs use them), there is no screen. The world
  confirms that choice at load and after every death, and ignores the
  screen's team, spawn and cancel commands. The pause menu's RESPAWN still
  works: a normal death, after which the choice is confirmed again.
- Otherwise the screen runs: the player starts with no unit. This is every
  battle the front end launches.
- `spawnscreen` (1 or 0) overrides either way.

The class played first:

- `unit=` names it.
- `side=` alone picks that side's first non-hero class in the mission.
- With neither, the screen's default is the initial class of the team that
  fields the old default unit, `rep_inf_jet_trooper`. On Theed that is the
  clone trooper on the Republic; a mission without it uses team 1.
- A bypass without either (`spawn=` alone, or `spawnscreen=0`) keeps the old
  default unit itself.

The team is the mission team whose roster lists the class, or whose hero it
is, else team 1. Without `side=` the side is the class's own.

## Phases

The player's phase:

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
camera"; `DeathCamera.update`, ported in
`rust/bfe-presentation/src/cameras/world_death.rs`) takes over from whatever view he
had, looks down on the body, and from 1.5 s turns to the killer.

- It is the world's camera, as the original's camera is what its AI
  scheduling reads. So it steps from world state alone; the local world
  thread, a server and replays get the same one. It also names the killer
  in plain numbers, which the client's death shot reads.
- The drawn camera is the client's own
  (`rust/bfe-presentation/src/cameras/death.rs`, the Death Camera option:
  Enhanced or Classic), from the body and the killer as the client draws
  them, until the screen's camera shot covers it or he spawns. Online, where
  the client takes no damage itself, the server's damage and death records
  name the killer.
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

- Only these values are kept; anything malformed is dropped by the client
  or rejected by the world.
- The queue holds at most one entry of each kind: a
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

`PlayerBodies` (rust/sim (player-body)) keeps one body per class. Each body is:

- a bot's body (`buildBody`): the model re-skinned by `OverrideTexture`, a
  `Soldier` or a `Droideka`, and its animator;
- what only the player's body has: `PlayerCombat` aimed from the world's
  camera with the HUD, the player's first-person arms (the droideka's with its own
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
  to load is left out of the roster. When his unit is chosen in advance,
  which can never change, only that class is loaded (and the bots' own, when the AI
  runs).
- The rosters are fixed once the map has loaded, and kept: the screen asks
  for the state every frame.

The world gives him another body only while he has no unit, from the spawn
itself, so the body always matches the class spawned. His unit keeps its
id, generation, team and Character. With the new body:

- the old body is hidden; its bubble, dust and jet flames stop; its arms
  leave the player's first-person view;
- the new body takes the old one's place and state: dead, at the same spot;
- the body's class, kind, hero flag, root scale and hit shapes, its
  animation, combat, items, jetpack, droideka shield and health type are
  the new class's;
- both soldier cameras take the class's sections and fields of view;
- the vehicle driver and the turret gunner, the look preview, water
  contact, the player's sounds and the perspective follow it.

Recon droids already out keep flying after he changes class; they know every
side's fire points.

### Soldier movement profiles

The simulation selects a profile for the battle before it builds any soldier
movers. Changing families after allocating movers requires a fresh world.
The `swbf1` profile uses the existing standing, crouching, prone,
jump, sideways dive and jetpack solver. Store soldiers and prediction use the
same profile entry and return its firearm permission for the tick. The
80-word movement exchange reports missing profile data for a `swbf2` mover.

The replay checkpoint for `swbf2` needs the exact stamina balance, exhaustion
flag, sprint ramp time when active, and roll direction and elapsed time
when active. Authority and prediction also need the equipped action and
class tuning. A zero-length step validates these inputs and preserves the
body, its look history and its timers.

The `swbf2` profile takes resolved class settings and state for one soldier
life. A standing soldier holding sprint walks until his forward velocity
exceeds 90% of maximum speed. Starting requires stick length at least 0.9,
positive forward input and absolute side input below 0.577 times forward.
The sprint row must be faster than the standing row. Holding sprint keeps
the same stick length requirement, widens the steering limit to 1.732 times
forward and requires forward speed above 0.2. The forward target rises from
maximum speed to the class sprint target over its acceleration time. The
exact time the body takes to reach that speed is still provisional.

A positive stamina cost can overdraw the remaining balance. Overdraw latches
exhaustion, even on a class whose floor is zero. Spending the exact balance
leaves the latch clear. Ordinary recovery runs while standing or crouching
on the ground. It uses the idle rate below horizontal speed 1 and the moving
rate otherwise. Reaching the class recovery threshold clears exhaustion.
A soldier still running forward with sprint held resumes automatically.

An eligible roll press takes precedence over crouch, jump and sprint. It
requires a strong forward or side input. A backward diagonal needs side
input greater than 0.839 times the magnitude of the backward input. The
class roll cost is paid on entry. The boost acts on existing forward and
side motion, and the class roll row handles steering thereafter. Travel
depends on incoming speed, inputs and collisions. The roll ends when elapsed
time exceeds three quarters of the equipped action's duration, using
(frame count minus one) / 30 seconds. It requests standing on expiry. A
soldier who released the stick can still coast as the ground brake takes effect.
Rolling uses crouched presentation and provisional crouch contact geometry.
The profile exposes that posture for future combat hit tests. Sprint and
roll restrict firearm use through the final active movement step.

Ordinary jumps add a vertical speed of sqrt(2 × 18 × JumpHeight). Unsupported
flight loses 18 units of vertical speed per second. Positive forward momentum
uses the forward jump factor. Sideways and backward momentum use the strafe
factor. Forward travel can continue after landing until the ground brake
brings the soldier to rest. From crouch, jump requests standing and checks overhead clearance.
A subsequent jump press can launch. From standing, crouch takes precedence
when both buttons are pressed. Sprint accepts jump and ignores crouch.
The PC prone command has no effect. Classes can supply jet controls and acceleration and draw
from a fuel tank or the stamina bar. A fully funded energy-jet step gets
thrust through its end. An underfunded step enters a fall and advances in
the same tick. Horizontal jet controls affect movement in the forward and
sideways directions. Vertical hover climb uses the unscaled forward stick.

An equipped combat action can supply a movement override for the tick.
Its input mask filters movement axes, turning and posture buttons before
the mover handles them. Authored planar velocity replaces stick drive,
braking and slope pull before displacement. A locomotion lock can also
carry the current velocity. The mover applies vertical impulse, gravity
and any fall-speed limit before displacement, then resolves contacts.
Movement and combat share the same stamina balance and exhaustion latch.

Local and dedicated admission select `swbf1` at the shared content stage.
Changing the family with retained movers requires a fresh world. Prepared class tuning, live
`swbf2` inputs and state, pose and combat integration, and multiplayer
prediction and publication are planned.

## Switching team

A team switch applies only while he has no unit, and only between
teams 1 and 2. No ticket is spent: the death already cost one. There is no
balance rule; team capacity is 200, as offline.

Switching moves every place the team is kept, together:

- with the Classic AI, its roster first, which moves the Character and
  clamps his class and post. His place in his post's queue is given up
  under the old team (its own post or an ally's) and taken at the new
  team's post. If it refuses, nothing changes. The bots on his new side
  then use the player curves, the living ones included. Without it, the
  Character's team.
- the player's team and the enemy team for friendly fire, the HUD's
  relations, items, kill messages, Victory or Defeat;
- the holograms' and maps' relation colours;
- the HUD's bars;
- the repair and lock-on default;
- every body's combat team: rounds, mines, droids;
- music and voice-over.

Rosters become uneven: the human is one of his team's `SetUnitCount`. The
original doesn't rebalance either.

## The spawn port

The screen reads the world only through the spawn port (`SpawnPort`,
`rust/bfe-battle-ui/src/spawn/model.rs`; the client's is in
`rust/bfe-client` `app/battle.rs`):

| Call | Notes |
| --- | --- |
| `human_state` | Phase, team, class, post, whether he may switch teams, and whether the battle is over. The class is that of his current or next life; after a team switch it is clamped into the new roster, as the Character's index is. The post is the last he spawned at, by name. |
| `team_facts` | Each team's side, logo, objectives and reinforcements. The side comes from map.json `spawnScreen`, else `SetTeamName`, else the initial class's side. |
| `roster` | Non-hero classes, in mission order, that loaded, with their display names (falling back to the class id) and weapons in slot order. |
| `posts` | Every post, with whether the team may spawn there (the Classic spawn manager's relations when it runs, else the registry's) and its vehicles (map.json `spawnScreen.postVehicles`). |
| `choose_post` | A post name or none: the previous one while it is eligible, else the eligible post nearest it, else the front line (`choose_frontline_post`). |
| `request`, `suicide`, `cancel` | Queued as human commands. |
| `camera_shots` | map.json `spawnScreen.cameraShots`. |
| `set_view_shot` | Sets the drawn camera every frame, after the soldier camera and its shake, until it is cleared; the sound's listener follows. The world's camera is never touched. Changing the shot resets the temporal upscaler's history. |

## Command vehicles

A command vehicle is a post while it lives, as in the original
([formats.md](formats.md), "Spawning and the spawn screen"): Hoth's two
AT-ATs, Geonosis' two AT-TEs and Rhen Var Harbor's AT-AT or AT-TE. Every
class on the `commandwalker` or `commandhover` base qualifies (the
MTT and the hovernaut too, which no stock map places).

- **The posts:** `CommandPosts.attachVehicles()` makes one post per vehicle
  pad whose classes include a command vehicle, named by the pad's slot
  (`transport-<slot>`, the names the network registry already reserves,
  `rust/sim` (`wire`)). `CommandPosts.all()` lists them
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
  it resets the no-post count (`rust/sim/src/battle/rules.rs`); its weights make it an
  objective (`rust/sim/src/ai/control/goals.rs`).
- **Lost:** when it dies the world reports it once: the AI's bots sent there draw again, its
  last attacker earns a post point unless he is the owner's friend, and
  "EMPIRE LOST THE AT-AT" is shown. A client showing a server's battle raises
  that message from the hulls it shows. A hull an enemy takes reports a
  change of owner to the AI.

Everywhere a spawn is chosen reads every post with the landed transports:
the screen (through the spawn port), the world's spawns for every human
(single player included) and the Classic spawn manager. On a client showing
a server's battle the posts come from the hulls it shows, and whether they
serve from the server's eligibility.

## Checks

`cargo test --locked -p sim -p bfe-battle-ui -p bfe-client` (in `rust/`)
covers the bypass rules; the phases and command rules, with AI off and with
a real Classic spawn manager; that every team holder moves (or none does);
the post rules; command vehicles as posts (which classes, live and dead
hulls, the owner, the spawn points and their pull-back, the events, the
bleed and the no-post count, a bot's spawn, the screen's choice and the
objectives); and command validation, the merge and the history.

## Gaps

- **No team-switch lock:** team switching is always allowed (Instant
  Action). The campaign modes' lock isn't modelled.
- **HUD while he has no unit:** the HUD still shows the last body's weapons
  and an empty health bar. Hiding them while the screen is open is the
  screen's job.
- **Fidget:** the fidget clip is approximate, as above.
