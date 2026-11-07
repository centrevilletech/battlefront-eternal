# Classic AI: implementation contract

This file is the working contract for the **Classic** AI in this repository,
the 2004 game's bot behavior written in our own code: the runtime objects the
AI reads and writes, the tick order, and the extracted data. Enhanced AI
(`?ai=enhanced`) is a separate mode built on the same native machinery; see
[ai-enhanced.md](ai-enhanced.md).

Rules that apply to every change:

- Behavior is the original game's, written in our own code. Never paste
  retail code or retail binary tables into the repo; implement behavior in Rust
  from its described semantics.
- Retail-derived output (anything generated from `data/game/`) goes under
  `public/maps/`, `public/sides/`, or `public/common/`, all gitignored. Tracked
  test fixtures are synthetic.
- Keep labels honest: where an implementation is provisional (a behavior not
  known exactly, or a stand-in until a dependency lands), say so in a comment
  and in the "Provisional" list at the end of this file. Do not ship an
  Enhanced-style substitute under a Classic label.
- Classic is asymmetric (player team vs enemy team skills differ). Keep it.

## Area documents

The [native ownership contract](sim-ai.md) describes the current implementation
in `rust/sim/src/ai/` and its generated host boundary. Each area below records
the behavioral contract; the Code column names the Rust modules that own it.
The bot code was ported from JavaScript; object-shaped API examples in these
documents describe the data each operation reads and writes.

| Area | Document | Code |
| --- | --- | --- |
| Multi-actor foundation: units, registry, battle, simulation events, the AI entry point, the shared random numbers | this file | `src/units.js`, `src/registry.js`, `src/battle.js`, `src/sim-events.js`, `src/ai-world-load.js`, `rust/sim/src/ai/owned/`, `rust/sim/src/ai/random.rs` |
| Navigation data and runtime | [ai-nav.md](ai-nav.md) | `rust/sim/src/ai/nav/`, `tools/zero_ai.py` |
| Perception, aim, firing, attack patterns, weapon AI, class and weapon AI extraction | [ai-combat.md](ai-combat.md) | `rust/sim/src/ai/combat/` (`perception.rs`, `aim.rs`, `agent.rs`, `weapons_ai.rs`), `tools/zero_ai_classes.py` |
| Difficulty, spawning, class choice, objectives (command post selection), mission and command post extraction | [ai-population.md](ai-population.md) | `rust/sim/src/ai/control/difficulty.rs`, `rust/sim/src/battle/spawning.rs`, `rust/sim/src/ai/control/goals.rs`, `tools/zero_ai_mission.py` |
| Vehicles and turrets: seats, boarding, vehicles as targets, Classic vehicle AI | [ai-vehicles.md](ai-vehicles.md) | `rust/sim/src/ai/vehicles/`, `src/vehicles.js`, `src/gunner.js`, `src/seats.js` |
| Class jobs: sniping, mines, pilot repair and resupply, scouts, jet hints | [ai-classes.md](ai-classes.md) | `rust/sim/src/ai/control/classes.rs`, `rust/sim/src/ai/nav/jet_hints.rs` |
| HUD toward other units: radar icons, reticle colour, hit ring, kill messages, enemy health, stats | [ai-hud.md](ai-hud.md) | `rust/bfe-render/src/hud_model/`, `src/hud.js` |
| Controller FSM, scheduler, source goals, interrupts, agents, patrol, squads, the Classic entry point | [ai-controller.md](ai-controller.md) | `rust/sim/src/ai/control/` (`mod.rs`, `controller.rs`, `scheduler.rs`, `policy.rs`, `agents.rs`, `patrol.rs`, `squads.rs`) |
| Heroes and melee | [ai-heroes.md](ai-heroes.md) | `src/heroes.js` |
| Enhanced AI | [ai-enhanced.md](ai-enhanced.md) | `rust/sim/src/ai/combat/enhanced.rs`, `rust/sim/src/ai/control/difficulty.rs`, `rust/sim/src/ai/control/goals.rs` |

## Runtime model

### Characters and units

The original keeps a fixed set of **Characters** per team (team slots, from
`SetUnitCount`); each Character spawns **entities** of a chosen class. The
port mirrors that split:

- `Character` (`src/battle.js`): `{ index, team, human, classIndex, unit,
  controller, spawn: { wave: S, ordinal: J, post }, kills, deaths, teamKills, captures }`. Persists for
  the match. `human` is true for the local player's Character. `unit` is the
  current living entity or `null`.
- `Unit` (`src/units.js`): one living (or dying) entity. It owns physics,
  combat, body, animation and presentation. Units are pooled per class and
  reused across spawns; `generation` increments on every spawn so stale
  references can be detected.

AI code reads units through this interface (fields are live; vectors are
shared, never mutate them):

```js
unit.id            // stable integer for the pooled unit
unit.generation    // increments on every spawn; refs store {unit, generation}
unit.team          // 1, 2 (mission teams); 0 neutral; locals may use 3+
unit.character     // owning Character or null
unit.classDef      // units.json class record (MaxSpeed, UnitType, weapons, ...)
unit.className
unit.kind          // classifier ID:
                   // 0 soldier, 1 droid, 2 walker, 3 droideka, 5 flyer, 6 hover,
                   // 7 mounted turret/passenger slot, 8 armed building, 9 building, 10 item
unit.alive         // spawned, not dead
unit.dead          // dying or dead (corpse still in the world)
unit.human         // driven by the local player
unit.hero          // hero class flag
unit.soldier       // Soldier or Droideka physics: position, velocity, aimYaw, aimPitch,
                   // posture, air, grounded, dive, lock, maxSpeed, maxStrafe, maxTurn, ...
unit.combat        // Combatant: health, maxHealth, dead, arsenal, team, ...
unit.position      // === soldier.position
unit.velocity      // === soldier.velocity
unit.aimPoint(out) // the point others aim at: body centre
unit.origin(out)   // controller position: the eye / gun pivot
unit.forward(out)  // controller forward: the aim direction, unit length
unit.sphere        // { center: Vector3, radius }: broadphase sphere, posture dependent
unit.intent        // ActorIntent this unit consumes next tick (below)
unit.controller    // Classic UnitController, or null (human)
unit.seat          // vehicle/turret seat it occupies, or null
```

Port conventions: world coordinates are the original's (no axis flip; the
extractor writes positions unchanged). Yaw is about +Y with forward
`(sin yaw, 0, cos yaw)`. A positive `turn` analog decreases `aimYaw`.

### Registry and relations

`world.units` (`src/registry.js`):

```js
units.all()                         // every pooled unit (alive or not)
units.living()                      // alive units
units.byId(id)
units.query(center, radius, out, filter?)   // alive units whose sphere meets the ball
units.resolve(ref)                  // {unit, generation} -> unit, or null when stale/dead
relation(teamA, teamB)              // 1 friend, 0 neutral, -1 enemy (mission relations)
```

### ActorIntent

What a controller writes for its unit each tick. The unit applies it in its
own tick, then clears the edge fields.

```js
intent = {
  forward: 0, right: 0,     // movement analogs, [-1, 1] (strafe right positive)
  turn: 0,                  // yaw analog: rate = turn * class MaxTurnSpeed (rad/s);
                            //   [-1, 1], or [-2, 2] with the transient facing request
  pitch: 0,                 // pitch analog: rate = pitch * class pitch coefficient
  jump: false, crouch: false, prone: false, use: false,     // edges
  buttons: [0, 0],          // per channel run-length button count;
                            //   > 0 held, < 0 released; the combat adapter turns
                            //   count transitions into weapon {held, pressed}
  reload: false, nextPrimary: false, nextSecondary: false, zoom: false,   // edges
  aim: null,                // Vector3 or null: the filtered aim direction weapons fire
                            //   along (the Classic Aimer output). null fires along
                            //   the body's aim (aimYaw/aimPitch)
  jetDirection: null,       // Vector3 or null: a jump jet's launch direction as the
                            //   path asks (Soldier.startJet uses it as given)
  exit: false,              // leave the vehicle or turret (edge)
}
```

Movement and aim go through the same `Soldier.tick` a human uses; bots never
set world position to follow a path.

### World services for AI

`ClassicAI` receives one `world` object:

```js
world.time                // simulation seconds since the match started
world.random              // ClassicRandom: generator A, the AI's decisions (route picks,
                          //   roulettes, timers)
world.randomB             // ClassicRandom: generator B, the game's own (squad rolls,
                          //   evade cubes, recovery, jet retries).
                          //   world.gameRandom is the same object (Nav's name)
world.collision           // CollisionWorld: cast(origin, dir, far, hit, ignoreBody, staticOnly),
                          //   castMoving(...), ground(x, z, top, out), overlapsSphere(c, r)
world.los(from, to, a, b) // true when the segment is clear for sight: terrain, static and
                          //   prop geometry, vehicles and soldiers (mask 0x4e), excluding
                          //   units a and b
world.units, world.relation   // above
world.posts               // CommandPosts: posts[] with .team, .position, .desc, AI props;
                          //   owned(team), at(point)
world.battle              // reinforcements per team, over, ...
world.mission             // ai-mission.json (below), plus map.json's mission block
world.nav                 // navigation data and services (ai-nav.md)
world.difficulty          // ClassicDifficulty (ai-population.md)
world.camera              // the human camera (scheduling LOD gate only; never a sensor)
world.events              // simulation event journal (src/sim-events.js)
world.spawnUnit(character, className, post, point, yaw)   // foundation spawns a unit;
                          //   point is [x, y, z] (feet, settled to the floor by
                          //   Soldier.spawn); returns the unit
world.spawns              // map.json spawn paths: name -> [[x, y, z, yaw], ...]
world.factory             // UnitFactory: sideOf(className), prepared(className)
world.classesFor(team)    // the team's [[className, count], ...] that a loaded side has
```

`createNativeAI(mode, world, options)` (`src/ai-world-load.js`) loads
`ai-nav.json`, `ai-mission.json` and each side's `ai.json` (`src/ai-data.js`
`loadAIData`), hands that cold content and the mission calls to the native
owner (`src/ai-world.js`, `rust/sim/src/ai/owned/entry.rs` and `load.rs`),
builds the initial roster, and returns an object whose `prepare(dt, tick)`
writes the AI's inputs for the next tick (for `enhanced` the native owner runs
the Enhanced pieces, [ai-enhanced.md](ai-enhanced.md)). Each simulation step,
`Game.step` (`src/game.js`) calls `ai.prepare` and then the native world tick
(`world_step_run`, `rust/sim/src/world/step.rs`), which runs the AI phase
before the unit phase. `update(dt, tick)` runs the AI phase alone, for tools
and tests. The mode,
difficulty and heroes are Options settings; URL parameters override them:
`?ai=classic|enhanced|off`, `?tier=easy|medium|hard`, `?seed=`,
`?heroes=1`.

Character fields in use: `index, team, human, className, unit, controller,
objective, kills, deaths, diedAt`. At death the Character drops its unit at
once (`character.unit = null`); the corpse lingers `CORPSE_LINGER` (4 s,
`src/human-spawn.js`) and goes back to the factory's pool. The human's
Character keeps the player's unit (`game.playerUnit`) and `src/game.js`
respawns him as before.

### Simulation events

`src/sim-events.js` is a bounded journal with independent subscriber cursors.
Gameplay emits once; AI and any other consumer read it. Audio keeps its own
`emitSound` hook; AI never replaces that listener. Records are immutable value
snapshots (positions copied):

```js
{ seq, time, kind, source: {unit, generation} | null, team, position: [x,y,z], ...fields }
// kinds: "shot" (weapon fired: weapon, direction, target ref when known),
//        "damage" (target, amount, attacker), "death" (unit, attacker),
//        "explosion" (def, radius), "grenade" (first physical contact: def, owner),
//        "footstep"/"land" (when simulation produces them), "capture" (post events)
```

### Tick order

Per simulation step `h` (the `step(h)` loop in `src/game.js`):

1. The AI phase (`ai_world_phase`): the Classic scheduler ([ai-controller.md](ai-controller.md))
   runs the budgeted subset of UnitControllers: phase 0, perception (scan,
   acoustic broadcast), phase 1; then the deferred-LOS budget and the
   path-planner service. Controllers write requests.
2. AI input execution for every AI unit (the original's AI input thread): the
   controller's movement request and aim state become the four analogs; fire
   requests become button counts; this fills `unit.intent`.
3. Every unit ticks: `soldier.tick(intent, h)`, then its combatant consumes
   the weapon part of the intent.
4. Ordnance, items, vehicles, capture, battle, as before.

The human keeps the per-frame input path; the scheduler does not run a
controller for the human Character.

The native owner keeps navigation (`Nav`, `rust/sim/src/ai/nav/mod.rs`),
difficulty (`rust/sim/src/ai/control/difficulty.rs`), spawning (whose
Characters become `battle.characters`, `rust/sim/src/battle/spawning.rs`),
the mission controls, the deferred line-of-sight queue
(`rust/sim/src/ai/combat/perception.rs`) and each side's class data itself;
the page's AI object exposes only the spawning view and the navigation
boundary. VO is written to
`world.events` as `kind: "vo"` records. Soldier and Droideka physics carry
`avoidance`, the near-horizontal building/prop contact normals summed since
the path executor last consumed them.

## Classic AI architecture (interfaces between areas)

Names follow the original game's class names.

### UnitController (Controller)

One per AI Character. Holds the FSM (current/pending state, multi-transition
drain), the CommandFsm source-goal queue (five records), the current agent,
and the fields other areas read or write:

```js
controller.unit                 // the unit it drives (null between lives)
controller.state                // 1..11
controller.agent                // current Agent
controller.request              // MovementRequest (below), written by the movement helper
controller.aimState             // 0 target (+lead), 1 stored point, other: along steering
controller.aimPoint             // Vector3 for aimState 1
controller.target               // weak ref {unit, generation}: the firing target
controller.aimTarget            // weak ref: what aim state 0 leads; separate from target
controller.fireA, fireB         // firing requests (A direct, B validated)
controller.facing               // transient enhanced-facing request
controller.perception           // Perception state (Combat): target heap, acquired target,
                                //   alerts (danger, targeted shot, acoustic refs), memory
controller.difficulty           // side curves (from world.difficulty, Population)
```

### Agent (Controller defines the base; Combat implements UnitCombatAgent)

Created on state entry, owns a movement helper (GroundGotoHelper, or
FlyGotoHelper for flyers).

```js
class Agent {
  constructor(controller, goal)   // goal: the source-goal record that started it
  tick(dt, phase)                 // per-state work, then helper.tick(dt)
  behave(dt)                      // phase-0 behavior
  exit()                          // state exit cleanup
  acceptsHazard()                 // priority 1 (grenade danger)
  acceptsAlert(known, point)      // priority 2: shot/damage alert
  acceptsTarget(unit)             // priority 3 (scanner target)
  acceptsAcoustic(unit)           // priority 4
  acceptsCommand(command)         // priority 5
  acceptsBoard(unit)              // priority 6
  acceptsSecondaryWait()          // priority 7
  acceptsRecovery()               // priority 8: stagnation recovery
  savesOnInterrupt(goal)          // interrupt saves the current command
}
```

An agent running a behavior helper (patrol, snipe, mine) delegates these
policies to the helper, so every helper implements them too.

### Movement helper and request (Nav)

```js
helper.goto(point, options)     // plan a route to point (graph route + local barrier path);
                                //   options.mode: "route" | "direct" | "straight". It replans
                                //   and stops the unit until the path arrives: call it only
                                //   when the destination changes
helper.stop()
helper.tick(dt)                 // advance path executor, write controller.request
helper.arrived                  // reached the destination
helper.status                   // executor status: 0, 1 no-improvement, 2 stationary
helper.failed                   // no route

MovementRequest = {             // the steering target the helper builds each tick
  valid, steeringTarget: Vector3, waypoint: Vector3, segmentDirection: Vector3,
  mode, lower: -1, upper: 1,
}
```

Request-to-analog conversion (the controller's request becomes the real
analog input) is `Nav::navigation_input` (`rust/sim/src/ai/nav/goto.rs`:
the executor step, analogs, jet, posture; `analogs` in
`rust/sim/src/ai/nav/analogs.rs` does the conversion alone). The class coefficients are MaxSpeed, MaxStrafeSpeed,
MaxTurnSpeed and MaxPitchSpeed, with no sign flips into the port's analogs.
Combat supplies the aim point for aimState 0 (target plus lead).
`controller.navControl` (NavControl) holds a unit's path state and its
`MovementRequest` (`controller.request`).

### Perception, aim and firing (Combat)

- `CombatAi::scan`, the deferred-LOS service `CombatAi::service_los`
  (`rust/sim/src/ai/combat/binding.rs`), and acoustic/danger/targeted-fire
  alert intake; called by the Controller's scheduler in tick order.
- `Aimer`: the direction filter (curves A/D from difficulty, halved for
  non-cannon weapons; hero exemption) producing `intent.aim`.
- Weapon AI: spread scale B on SpreadPerShot, sniper extra yaw S, the AI
  spread floor (`Weapon` with `{ player: false }`).
- Button run-length objects and the semiauto/burst/charge/sniper firing
  discipline producing `intent.buttons`.
- `UnitCombatAgent` (state 2) and `AttackPatterns`.

## Extracted data

All under gitignored `public/`. Each file records `schema` (integer),
`generator` (tool name and version), and source archive hashes.

- `public/maps/<map>/ai-nav.json` (+ `ai-nav.bin` if typed tables are large):
  planning graph (hubs, arcs, packed routing tables, capability masks),
  hints (type, transform, properties in source order), barriers, boundary,
  regions and paths the AI uses. Schema in `docs/ai-nav.md`.
- `public/maps/<map>/ai-mission.json`: every AI-relevant mission call in
  source order with arguments (difficulty setters, SetUnitCount,
  AddUnitClass, SetHeroClass, SetSpawnDelay, SetTeamAggressiveness,
  SetDenseEnvironment, SetStayInTurrets, SetAIVehicleNotifyRadius, snipe
  ranges, SetAttackingTeam, ...), plus per-post AI properties (Value_ATK_*,
  Value_DEF_*, *Ban, SpawnPath/AllyPath, class spawn counts). Schema in
  `docs/ai-population.md`.
- `public/sides/<side>/ai.json`: per-class AI properties (UnitType role,
  AISizeType, hero flag inputs, VO banks) and per-weapon AI properties
  (Min/Optimal/MaxRange with implicit-range flag, Target* masks, class-type
  tokens such as cannon/launcher/grenade). Schema in `docs/ai-combat.md`.

Standalone tools write only these files, so refreshing AI data never rewrites
a map's geometry: `python3 tools/zero_ai.py nab2` and so on. A normal
extraction (`tools/extract.py`) writes them too.

## Tests

- `node --test tests/ai-*.test.js`: the host boundary and native owner
  with synthetic fixtures (tracked); checks against optional local reference
  data skip when it is absent.
- `cargo test` in `rust/sim`: the AI's native tests (`tests/ai_phase.rs`,
  `ai_native_phase.rs`, `ai_combat_control.rs`, `ai_control_twin.rs`,
  `ai_combat_twin.rs`, `vehicle_ai_twin.rs`, the replay suites, and
  `src/ai/nav/tests.rs`).
- Whole battles between bots (`tests/ai-browser.mjs`) run headless under
  Node (`tools/headless-world.mjs`), the world the world worker runs, and
  read `game.ai` there.
- `tests/ai-browser.mjs` fails a battle where too many bots never leave
  their spawn. The measuring probes `tools/ai-stuck-probe.mjs`,
  `tools/fire-pace-probe.mjs` and `tools/ai-focus-probe.mjs` read the former
  JavaScript AI's state and do not run against the native AI; the results quoted in these docs were measured with them before the port.

## Status

Classic services are included in the production bundles, with stock heroes,
match options and squad input. Supporting APIs and exact fidelity limits live
in [ai-vehicles.md](ai-vehicles.md), [ai-classes.md](ai-classes.md),
[ai-hud.md](ai-hud.md) and [ai-heroes.md](ai-heroes.md).

Options, Game offers BATTLE AI (OFF, CLASSIC, ENHANCED), DIFFICULTY (EASY,
NORMAL, ELITE), INSTANT ACTION HEROES and FRIENDLY FIRE. AI, difficulty and
hero changes take effect through Restart battle; friendly fire applies
immediately. URL overrides remain supported. The default squad keys are F1
At ease, F2 Follow, F3 Move out and F4 Hold. The default battle AI is Off.

## Stuck bots

Play-testing found large groups of bots that stopped moving, offline and on
hosted servers. `tools/ai-stuck-probe.mjs` reproduced it before the AI moved
to Rust (it does not run against the native AI): whole Classic
battles in plain Node (the world worker's and the dedicated server's code,
faster than real time), every bot sampled once a game second. A bot is
stuck when it has lived a whole 10 s window without leaving a 1 m ball and
was not holding still on purpose: in a turret or passenger seat, at a snipe
hint, at a patrol hint or the roam's wait, under a hold order, in the crew
wait, standing in the capture region of a post it is taking, or having fired
in the window. A stuck driver counts by its hull. The probe printed stuck
bot-seconds per minute, the most stuck at once, and the stuck bots grouped
by AI state with examples (goal, queue, agent and helper states, path
executor, planner slot, target, job, posture, contacts):

What kept bots standing, all in the shared runtime that single player (page
or world worker) and the server both run (the flyers' pads are tested in
`tests/flyer-pads.test.js`), details in the area docs (paths under
`rust/sim/src/ai/` unless given in full):

| Where | Cause | Change |
| --- | --- | --- |
| `control/classes.rs` repair service | A pilot's walk to a wreck was planned again every update: each `goto` restarts the plan and stops the bot, so the path was thrown away before use. Over a third of all planner work on Cloud City, which left everyone else's paths waiting | the walk is planned once ([ai-classes.md](ai-classes.md)) |
| `control/classes.rs` repair service | With no lock, `undefined === undefined` made every job "close" inside the cutter's default 60 m: pilots stood far off cutting at nothing; routes end outside the cutter's reach; the same futile job was picked again every 5 s | lock needed; close in straight; give up after 8 s; skip a given-up job 30 s |
| `nav/goto.rs`, `nav/control.rs` | A failed route leaves no executor, so status 0 and no stagnation recovery: the bot stood where the route failed (patrols, scouts, boarders); a fighter's look round between two failed routes from one spot started the count again | stranded: status 1 after the executor's 5 s, counted on while it stays within 2 m ([ai-nav.md](ai-nav.md)) |
| `nav/planner.rs`, `control/controller.rs`, `nav/goto.rs` | Strict priority let fighters' priority-0 asks hold route legs back for a whole fight; dead bots' searches kept their budget; a full queue left the helper waiting | starvation bound (20 searches); unbind drops a life's work; refusal is a failure |
| `control/controller.rs`, `vehicles/service.rs` | A mounted patrol published an objective while its goal started: 64 rebuilds a service and the walker never acted | publication starts at the next phase 1; the slow-walker branch excludes its own post |
| `nav/analogs.rs` | A Droideka at rest steered 0.2 m on, exactly its stop distance: rounding decided whether it ever started | stops only at the waypoint itself |
| `vehicles/steering.rs`, `vehicles/service.rs` | A walker near a waypoint behind it faced its aim and stood; touching bounding spheres renewed its yield hold beside a parked hull forever; a walker against a bank, or wedged by a wall facing away from its path, had no recovery; a hover or walker stopped to stand and shoot faced the point under it, not its target, and never fired a fixed gun | face the waypoint while a path is left; contacts need closing motion; the hover's reverse-and-turn, also for a hull that will not turn; standing on the point is arrival, so it faces its aim ([ai-vehicles.md](ai-vehicles.md)) |
| `vehicles/agents.rs` | A boarder whose walk ended under a turret on a deck above held its reservation forever | give up, no invitation for 15 s |
| `control/patrol.rs` | A patrol sent to an enemy post's center plus a box offset could end outside a round capture region (or below one on a deck, as at Yavin's temple top); state 2 has no timer, so it stood beside the post taking no part | on to the center; still outside 20 s later, another objective ([ai-controller.md](ai-controller.md)) |
| `control/agents.rs` recovery | A soldier in a hollow of steep slopes slid back from every random direction recovery tried | a timed retry that got nowhere jumps as it sets off |
| `combat/weapons_ai.rs` | A weapon whose masks reject the target (launcher vs turret) was chosen and never fired; a burst of exactly 1.0 heat never started: bots aimed in pattern 2 for as long as the target was in view | validation by the controllable's target, as the original; a burst that fills the gauge counts as 0.99 (port) ([ai-combat.md](ai-combat.md)) |
| `combat/agent.rs` patterns | Leaving cover (pattern 9) skipped the cover helper's exit, so the target stayed frozen in the tracker: a bot then in pattern 2 kept facing where an unseen target had been, for as long as it lived | the cover helper's exit runs first ([ai-combat.md](ai-combat.md)) |
| `rust/sim/src/flight/ai.rs`, `src/flight/model.js`, `vehicles/agents.rs` | A Bespin gunship's takeoff was blocked by pad structure reaching into a corner of the box its parked hull fills; its AI pilot held the key, and a takeoff is tried only on a fresh press, so it never tried again; 3 s later the stranded-flyer check blew it up on its pad with its crew, and it came back there to do it again | the pilot presses again every second; the takeoff and lift look past what the parked hull overlaps; a craft still on its pad is left by its crew, not destroyed ([ai-vehicles.md](ai-vehicles.md), [vehicles.md](vehicles.md)) |

Results, before and after these changes (5 minute battles at the medium
tier; the same probe, seeds 1 to 3 in single player and 1 and 2 on the
server world; a longest stand of about 280 s lasted the whole battle):

Single player, the player idle at his post:

| Map | Bots | Stuck bot-s/min before | after | Most at once before | after | Longest before | after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| nab2 | 40 | 342.9 | 4.4 | 11 | 2 | 287 s | 8 s |
| tat2 | 43 | 759.1 | 86.8 | 40 | 10 | 280 s | 150 s |
| bes1 | 48 | 175.7 | 26.0 | 8 | 3 | 278 s | 25 s |
| bes2 | 42 | 1483.1 | 28.4 | 46 | 3 | 277 s | 68 s |
| hot1 | 50 | 322.2 | 17.7 | 14 | 4 | 270 s | 94 s |
| kas2 | 50 | 521.7 | 62.3 | 20 | 5 | 280 s | 69 s |
| yav1 | 42 | 275.7 | 17.3 | 12 | 2 | 280 s | 168 s |
| geo1 | 49 | 274.4 | 6.6 | 10 | 2 | 233 s | 53 s |
| end1 | 43 | 263.0 | 18.7 | 12 | 3 | 280 s | 18 s |
| rhn1 | 44 | 259.6 | 8.4 | 10 | 3 | 278 s | 28 s |
| Mean | | 467.7 | 27.7 | | | | |

The server's world (`--mode mp`), 32 a side and no players:

| Map | Bots | Stuck bot-s/min before | after | Most at once before | after | Longest before | after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| nab2 | 59 | 328.7 | 2.7 | 9 | 2 | 285 s | 6 s |
| bes2 | 55 | 2594.5 | 91.5 | 63 | 7 | 279 s | 171 s |
| tat2 | 56 | 1206.0 | 91.9 | 54 | 8 | 280 s | 55 s |
| hot1 | 57 | 277.5 | 31.9 | 11 | 3 | 265 s | 224 s |
| kas2 | 57 | 549.9 | 46.4 | 19 | 4 | 280 s | 79 s |
| Mean | | 991.3 | 52.9 | | | | |

Remaining: most of what the probe still counts is fighters turning to look
after alerts (combat states 2 and 3, about 3 s a time). Fire within 30 m
alerts through walls, as Classic's does, and in a close town fight (Mos
Eisley) a bot is alerted again as soon as its orders resume, so it can look
round for 10 s and more; such bots make up the most stuck at once now.
Pilots give up a wreck they cannot reach (8 to 20 s), flyers wait on the
ground for their crew, and in the server's 64-bot battles some route
requests wait seconds for the planner. Not the AI's: a hover hull the
suspension's step test pins in place (`src/vehicles.js`: every move
reverted while a support probe sees a floor just over 1.1 m up; Yavin near
(40, -65, 0)), a tauntaun wedged in a corner (the push-out restores its
yaw, and there is no room to back out) and soldiers wedged in geometry
(never grounded, falling in place; Mos Eisley). Their drivers and walkers
keep trying until killed.

## Provisional

This is an implemented Classic mode, not a claim of complete parity with the
original game. Per-area documents retain the known/unknown distinctions and
remaining approximations. In particular:

- Full match comparisons against the original game and coupled-clock
  equivalence remain to be measured. Synthetic numerical fixtures and browser
  smoke tests prove their stated scope only.
- The original's entity-grid visiting order is approximated for capped
  broadphase queries. Class-job, melee and radar geometry use available
  runtime bounds.
- Custom self-piloted repair droids and custom CanDeflect=1 projectile rebound
  are unsupported. Stock recon is not incorrectly assigned repair behavior;
  stock sabers use the documented effective-zero defense profile.
- Hero capes, the original's saber glow flicker, and hero swing/hit sounds and
  defense poses are unfinished ([ai-heroes.md](ai-heroes.md)).
- The existing soldier/water service is shared by on-foot bots. The
  original's droideka-specific water behavior and strict water clock parity
  remain shared gameplay limitations; no Enhanced water avoidance is added
  here.
