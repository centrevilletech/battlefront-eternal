# Classic AI: controller, scheduler, agents, interrupts, patrol, squads

Controller part of the Classic AI (`docs/ai.md`). It covers the runtime
architecture and state dispatch, the controller collections and AI update
phases, clock and cadence, distance/visibility LOD and input execution, the
scheduling visibility gate, squads, followers, commands and VO, interrupt
priorities, patrol and objective execution, the class/helper architecture,
priority 8 stagnation recovery, command post objective destinations that
consume the CaptureRegion directly, and the scheduling compatibility
contract. Further behavior details are under "Further behavior details".

| File | What |
| --- | --- |
| `src/ai/classic/controller.js` | `UnitController` (controller FSM, CommandFsm queue, request/alert fields), `SourceGoal`, `CommandQueue`, the generic `Fsm` driver, `GOAL`/`STATE`/`ROLE`/`COMBAT_MODE` |
| `src/ai/classic/agents.js` | `Agent` (UnitAgent base), Wait, WaitSecondary, Trooper, Scout, Repair, Random, Follow, Hold, fallback Board/UnBoard and Snipe/Mine helpers, the degraded `DirectHelper`; runtime vehicle/class bindings replace the fallbacks |
| `src/ai/classic/interrupts.js` | `InterruptHelper`: priorities 1 to 8, push/save, the charge test |
| `src/ai/classic/patrol.js` | `PatrolHelper` (states 1 to 6 and 100), patrol-hint query and choice, command-post geometry |
| `src/ai/classic/squads.js` | squad commands, delivery, follower counting, command VO |
| `src/ai/classic/scheduler.js` | `ClassicScheduler`: budgeted controller services, LOD gate, the AI input step |
| `src/ai/classic/index.js` | `createClassicAI`, `loadClassicModules`, sibling adapters |
| `tests/ai-controller.test.js` | `node --test tests/ai-controller.test.js` |

The modules import only `three`, `src/ai/random.js` and each other.

## Using it

```js
import { createClassicAI, loadClassicModules } from "./ai/classic/index.js";

const modules = await loadClassicModules();       // dev convenience; a bundle should
const ai = createClassicAI(world, { modules });    //   import the siblings statically
// (without `modules` it loads them itself; await ai.ready before stepping)

for (const character of spawning.characters) ai.attach(character);   // humans are ignored
// inside world.spawnUnit, once character.unit is set:  ai.spawned(character)
// when a unit dies (with spawning.onDeath):            ai.died(character)
// per simulation step h, before units tick:            ai.update(h)
// the local player's squad buttons:                    ai.playerCommand(human, ai.COMMAND.FOLLOW)
```

`update(h)` = `decide(h)` then `execute(h)`:

1. `decide`: Combat's event ingestion (shots/damage become controller alerts),
   spawn/death sync for anything not reported, then the scheduler: cycle step,
   budgeted controller services (phase 0, deferred-LOS cleanup, sight scan,
   acoustic broadcast, phase 1, candidate clear), then the path-planner and
   deferred-LOS budgets.
2. `execute`: the AI input step for every live AI unit, filling `unit.intent`.

`spawned` and `died` are optional (`update` notices both), but call them to
keep the original game's RNG order: it builds a new controller per spawned
entity (drawing its roll) before Population republishes the objective.
Calling `spawned` inside `world.spawnUnit` matches `docs/ai-population.md`.

Debug: `ai.debug.snapshot()` (cycle, budgets, every controller's state,
agent, goal, queue, alerts) and, with `ai.debug.tracing(true)`,
`ai.debug.trace(unitId)` (the controller's last 64 goal/state/interrupt
events).

### World fields read

| Field | Use |
| --- | --- |
| `world.time` | every timer (seconds) |
| `world.random` | generator A (see Further behavior details); `world.randomA` wins if set |
| `world.randomB` (or `world.gameRandom`, Nav's name) | generator B; absent: shares A, with a warning |
| `world.units` | `query` (allies, recruitment), `relation`, `add` refs |
| `world.relation(a, b)` | relations (else `world.units.relation`) |
| `world.spawning.characters` / `world.battle.characters` | the roster (follower counts, delivery, humans for the acoustic cycle) |
| `world.difficulty` | `cycle`/`losBudget` (a `ClassicDifficulty`), else `hard`/`tier` |
| `world.nav` | a `ClassicNav`: `helper()`, `control()`, `planner`, `barriers`, `hints`, `service()`, `idle` |
| `world.camera` / `world.cameras` | the LOD gate only (`matrixWorld`, `near`, `far`, `fov`, `aspect`, `zoom`) |
| `world.events` | VO records (`kind: "vo"`) |
| `world.missionControls` | Population's `missionControls(calls)`: `formation`, `stayInTurrets`, `stationaryWait` |
| `world.objectives` / `world.spawning.objectives` | through Population's `classicBinding` |
| `world.lodDisabled`, `world.network`, `world.paused` | gates |

## Module APIs

### `controller.js`

```js
new UnitController(ai, character)
controller.bind(unit) / unbind()            // per life (index.js calls these)
controller.update(dt, phase)                // phase 0 or 1 (the scheduler calls this)
controller.state, pending, agent            // controller FSM: 1..11
controller.commands                         // CommandQueue: current, queue (<= 5), latch, currentDone
controller.publishObjective(post)           // source goal 3, replacing the queue (Population calls this)
controller.requestObjective(exclude)        // Population's request
// request fields (cleared each phase 0 where the original clears them)
controller.request                          // MovementRequest (Nav's NavControl replaces it)
controller.aimState, aimPoint, aimTarget    // 0 aimTarget (+lead), 1 aimPoint, 2 along the path
controller.target, fireA, fireB             // Combat's firing target and requests
controller.facing, stance, rangeScale, jet, wantedMode, exitRequest, dodge
controller.lookAt(p) / aimAt(ref) / lookAlongPath() / setStance(s) / requestFacing() / requestJet(v) / requestExit()
controller.leader                           // follow leader ref (what follower counts read)
controller.roll                             // integer 0..100 drawn per life (Combat reads it)
controller.navControl, perception, difficulty
// alert intake (Combat's producers); cleared after each phase 0
controller.notifyDanger(point)
controller.notifyShot(sourceCharacter, known, point)
controller.addAcoustic(ref)                 // at most 20
controller.addVehicleNote(ref)              // at most 10
controller.receiveCommand(squadCommand)
controller.danger / shot / acoustic / vehicleNotes / command / lastCommand
controller.executorStatus, hasExecutor, avoidSide   // read through navControl
```

`SourceGoal` records: `{ kind, refA, refB, post, point, mode }`;
constructors `objective(post)`, `combat(mode, { point, target })`,
`follow(leaderRef, sourceRef)`, `board(vehicleRef)`, `droidRepair(t, o)`,
`of(kind)`. `CommandQueue`: `overwrite`, `append`, `prependOne`,
`prependTwo(saved, goal)`, `popFront`, `removeKind`, `needsNextGoal`,
`currentCopy`. `Fsm`: `state`, `pending`, `requestState`, `update(dt,
alternate)`, `flush`, `terminate`, `done`, hooks `onEnter/onExit/onTick`.

### `agents.js`: the `Agent` base (UnitAgent)

```js
class Agent extends Fsm {
  constructor(controller, goal)   // goal: the SourceGoal; creates interrupts and helper
  tick(dt, phase)                 // controller-facing: update(dt, phase === 0); returns state === 100
  exit()                          // controller state exit: helper.terminate (or stop), then terminate()
  onEnter() / onExit()            // the agent's own state entry/exit
  behave(dt)                      // phase-0 per-state work; the base onTick calls it in phase 0
                                  //   and combat.rankTargets(agent) in phase 1
  // shared steps
  updateCombatTarget()            // Combat's full tracker update
  aim(stance, look?)              // posture request, then aim at the fresh target, else the
                                  //   remembered point, else `look`, else along the path
  tickHelper(dt), canEngage(unit), boardPolicy(unit)
  // policies
  acceptsHazard()                 // grenade/sticky danger
  acceptsAlert(known, point)      // targeted shot / damage (docs/ai.md's acceptsAlert)
  acceptsTarget(unit), acceptsAcoustic(unit), acceptsBoard(unit)
  acceptsSecondaryWait(), acceptsCommand(command), acceptsRecovery(), savesOnInterrupt(goal)
  onMoveOut()                     // move out / at ease side effect (crew wait, flyer)
}
```

Agents: `UnitWaitAgent` (1), `UnitTrooperAgent`/`UnitScoutAgent`/
`UnitRepairAgent` (3, by UnitType role), `UnitWaitSecondaryAgent` (4),
`UnitBoardAgent` (5), `UnitUnBoardAgent` (6), `UnitRandomAgent` (9),
`UnitFollowAgent` (10), `UnitHoldAgent` (11). State 2 is Combat's
`UnitCombatAgent` (a `MissingCombatAgent` that finishes at once stands in
when it is not bound).

The movement helper an agent drives (Nav's `GroundGotoHelper`):
`goto(point, { mode })` with mode `"route"` (default), `"direct"` (the
patrol's roaming), `"straight"` (the recovery's two-point path); `stop()`,
`tick(dt)`, `arrived`, `status`,
`destinationPoint(out)`, `setLeader(ref)`, `followTrail(on)`, `holdPath(on)`,
`installPath(points)`. Optional: `terminate()`.

### `interrupts.js`

`new InterruptHelper(agent)`; `tick()` runs priorities 1..8 and returns
whether one fired. When Combat's `hazardResponse`, `alertResponse` and
`acousticResponse` are bound they decide priorities 1, 2 and 4 (perception
math is Combat's); the helper still applies the shared gate, builds the
goal, and handles their `exit` (turret leaves) and `alert` (HeardEnemySound)
flags. Fallback implementations are used otherwise. `charging()` is the
charge test.

### `patrol.js`

`PatrolHelper(agent)`: `start(post)`, `tick(dt)`, `exitState()`, `look`,
`lookPoint`, `stance`, `hint`, `expires`. Helpers: `captureRegion`,
`cpCenter`, `cpOffset`, `arrivedAtObjective` (Population's
`objectiveCenter/objectiveOffset/objectiveArrived/postAI` when bound),
`queryPatrolHints`, `chooseHint`, `hintAvailable`, `markHintUsed`,
`pickHintStance`, `friendlySpawnEligible`.

Hint fields read: `bit` (or `kind`), `position` (or `xfrm[9..11]`),
`forward` (or `matrix`/`xfrm[6..8]`), `post`/`commandPost`, `stanceMask`
(PrimaryStance bits; default stand), `reservedAt` (default −10),
`linked`/`next` (radius siblings). Nav may instead provide
`queryHints({ mask, center, halfExtent, cap, requester, now })`,
`hintAvailable(hint, now)` and `markHint(hint, unit, now)`.

### `squads.js`

`COMMAND` (0 move out, 1 at ease, 2 hold, 3 follow, 4 board, 5 unboard),
`SquadCommand(source, kind, time, target)`, `sendCommand(ai, cmd, p)`,
`distributeCommand(ai, cmd)`, `playerCommand(ai, character, kind)`,
`countFollowers(ai, leaderUnit)`, `currentControllable(ai, character)`,
`voEntry(kind, context, human)`, `commandVo`, `emitVo`, `VO_BANK`,
`VO_CONTEXT`.

VO records go to `world.events` as `kind: "vo"`: `{ event, property, source,
team, position, human, ... }`. Events: `tac_moveout`, `tac_hold`,
`tac_follow`, `tac_needgunner`, `tac_spreadout`, `tac_stop`, `tac_allclear`
(command VO, with `command`, `context`, `positional`, `notice` for the local
player's HUD); `tac_yes` (acknowledgement, AISCResponseYessirSound);
`heardEnemy` (HeardEnemySound); `charge` (ChargeSound). Audio maps
`property` through the unit's class.

### `scheduler.js`

`candidateBudget(N, C, t)`, `schedulerConfig(hard)`, `cameraAccepts(camera,
point)`, `fallbackAnalogs(...)`, and `ClassicScheduler`: `update(dt)`,
`service(controller, dt)`, `lodSkip(controller)`, `visibleOrNear(unit)`,
`executeInputs(dt)`, `executeInput(controller, dt)`, `stats`, `counter`,
`parity`, `inputParity`, `skips`.

The input step per unit: the skip gate (identity parity against
`inputParity`, not visible, not skipped last time; a skipped step's dt is
added to the next), then `combat.consumeFire(controller, unit, step, intent)`,
`intent.exit` for an exit request, then Nav's `navigationInput(control, unit,
intent, step, aimPoint, { facing })` (the executor step, analogs, jet,
posture), then the droideka's wanted mode as a `use` press and Combat's
`controller.dodge` as a sideways jump. The aim point is Combat's
`aimPoint(controller, unit, dt, out)` (null for "along the path").

### `index.js` adapters

Each sibling is bound by an adapter that maps its published API onto the
names above; `options.nav`, `options.combat`, `options.population`,
`options.vehicles`, `options.classes` override per name, then a module's
`classicBinding(ai)`, then these defaults:

- Nav (`nav/index.js`, `nav/goto.js`, `nav/units.js`): helpers through
  `world.nav.helper(controller, world, { agent })` (else `GroundGotoHelper`/
  `FlyGotoHelper` as `new Helper(controller, world, { agent })`);
  `navigationInput`; `analogs` (fallback); planner `idle`/`service`;
  `barriers.pointClear`/`barriers.obstruct`; `traversalRadius`/`traversalMask`;
  `hints`.
- Combat (`perception.js`, `weapons-ai.js`, `aim.js`, `combat.js`):
  `Perception`, `TargetTracker` (as `agent.tracker`), `scan` (needs
  `dispatch`), one `DeferredLos` (`world.deferredLos`), `broadcastAcoustic`,
  `clearCandidate`, `thin` (phase-1 ranking), `sightRange`, `sensorRange`,
  `selectedWeapon`, `consumeFire`, `aimPointFor`, the three responses,
  `UnitCombatAgent`, and `classicBinding` (createPerception, attachController,
  ingest, aim, leadPoint, ...).
- Population (`goals.js` `classicBinding`, `difficulty.js`): `requestObjective`,
  `friendlySpawnEligible`, `curvesFor`, `isHard`, `postAI`,
  `objectiveCenter`, `objectiveOffset`, `objectiveArrived`.
- Vehicles (`vehicles/index.js`, [ai-vehicles.md](ai-vehicles.md)):
  `agents.{board,unboard,waitSecondary}` and services `boarded`,
  `boardedVehicle`, `boardedVehicleOf`, `boardedVehicleOfCharacter`,
  `currentControllable`, `isMountedTurret`, `emptySeats`, `crewControllers`,
  `seatsFull`, `pendingReservations`, `lowTickets`, `pilotKeepsSeat`,
  `alertsBlocked`, `mountReach`, `refusesHold`, `canEnter`, `freeSeats`,
  `reserve`, `reserveSeat`, `boardTick`, `boardPolicy`, `targetReserved`,
  `reserveTarget`, `patrolMounted`, `isAtAt`, `notify`, `requestExit`,
  `createHelper`.
- Class behaviors (`classes.js`, [ai-classes.md](ai-classes.md)):
  `helpers.{snipe,mine}` and `selectSnipe`, `canMine`, `selectMine`,
  `findRepairJob`, `startRepairService`, `repairService`.

## How it runs (summary)

- Controller FSM (states 1..11): entering a state builds its agent; exiting
  calls the agent's `exit()` and drops it. The CommandFsm keeps the current
  goal and up to five queued records. Phase 0 clears the request fields
  (fire requests, firing target, facing, jet, range scale), ticks only the
  current state (alternate mode; nothing ticks while a goal waits to start),
  then clears the one-update inputs (shot/damage alert and its known flag,
  danger, vehicle notes, pending command, acoustic list). Phase 1 starts a
  goal when the latch is set or the agent finished with goals queued
  (popping it, mapping kind to state), then drains state changes and ticks
  (the agents' phase-1 tick is the target-ranking pass); it repeats while
  another goal is ready. Agents run the same driver: phase 0 ticks their
  behavior, phase 1 applies the states they requested.
- Goals → states: 1..6 → 1..6, 8 → 9, 9 → 10, 10 → 11; 7 none (droid
  controllers only). State 3's agent by role: Scout (0) scout, Support/
  Repair/Special (3) and Pilot (4) repair, others trooper.
- Interrupts (every agent's behavior starts with them): 1 danger (soldier,
  weapon in hand, agent accepts) → combat evade at the point; 2 shot/damage
  → combat at the source's current position if within sensor range and
  engageable, else (soldier) evade to self plus a generator-B cube; 3 the
  deferred-LOS candidate → combat on it (droideka: within 10 m; flyer
  target reservations); 4 acoustic refs in order → combat at the first
  accepted within the horizontal hearing range (half for droidekas), with
  HeardEnemySound; 5 squad command; 6 boarding (one generator-A pick among
  the notes); 7 crew wait window; 8 stagnation (executor status ≠ 0) →
  recovery. Priorities 1 to 4 share a gate (a vehicle-class flag; flyers in
  danger boxes). Pushing: `savesOnInterrupt(goal)` → prepend [goal,
  copy of current], else prepend goal; refused while the latch is set.
- Objective agents: Trooper, state 1: arrived (zero offset) → patrol (2),
  else walk (3) with a "follow me" sent at 0.3 (VO only from an AI); 3: walk
  to center + offset; on arrival (or the helper's arrival) patrol and send
  "move out" (always); objective-moved retry; sniping (soldier, not hero)
  and mines; 4 snipe, 5 mines. Scout: no chatter, sniping
  checked first in 3, 60..100 s sniping deadline. Repair: 2 walk, 3 service,
  4 patrol, 5 snipe, 6 mines; job check every 5 s. Class jobs and tactical
  helpers are bound by `classes.js`; see [ai-classes.md](ai-classes.md).
- Patrol: see the header of `patrol.js`.
- Follow: approach the leader's position (state 2, horizontal 0.2), keep
  (3), stand off at a random flat offset up to the far radius (4); stop
  under (near + 5), resume over (far + 6); a stuck executor in 3 restarts
  the approach, in 4 re-installs a stand-here path; ends when the leader is
  gone, has more than two followers, boards (others than droidekas queue a
  Board for its vehicle), or after 20 phase-0 callbacks farther than 20.
- Hold: look 10 out (random flat direction; ±30° of home for stationary
  controllers), re-picked with a 1-in-9 draw each callback; fights; ends
  after 20 far callbacks from its issuer, or when the issuer has no control.
- Random recovery: a generator-B cube point ±10 per axis as a straight
  two-point path; done beyond 10 m (2 m droideka) when clear of barriers;
  retry on a contact sidestep, after more than 3 s, or with no executor.
- Squads: see the header of `squads.js`. Acceptance: the recipient's count
  (followers + 1) < 2 and the leader's < 3.
- Scheduler: see the header of `scheduler.js`.

## Further behavior details

- **Two generators.** Generator A draws the controller roll, Wait and Hold
  look angles, Hold and Patrol cubes, Patrol draws, command post offsets,
  hint choice and stances, the boarding pick, command post selection, class
  selection, the spawn point, the danger delay, line-of-sight thinning,
  repair job choice and fly landing. Generator B draws the squad-command
  roll, the shot-alert evade cube, Random and Follow cubes, the combat jet
  producer and the opportunistic jet retry.
- **Objective publication overwrites.** Publishing an objective replaces the
  command record (the queue count becomes 1) and sets the latch. The new
  objective therefore starts at the next phase 1 and drops queued goals
  (including a saved objective behind a combat goal). The capacity-checked
  append is a separate operation.
- **Pending test.** A goal is pending when the latch is set, or when the
  current agent has finished (set from the agent tick's return) and goals are
  queued. Starting a goal copies the front record into current and shifts the
  queue. Prepending a saved record and a goal stores the goal first and the
  saved record second. In phase 0 the FSM wrapper ticks only when nothing is
  pending.
- **Agent policies.** The base agent accepts hazards, alerts and commands;
  takes a scanner target or an acoustic contact only when weapon eligibility
  says it could engage; applies the board policy; has no crew wait; allows
  stagnation recovery; and saves the interrupted goal. Overrides:
  - Wait accepts the crew wait.
  - WaitSecondary rejects for flyers (closing its window); it refuses board,
    crew wait and saving.
  - Random accepts the crew wait; it refuses recovery and saving.
  - Follow refuses hazard, alert, target, acoustic, board and crew wait;
    it rejects a FOLLOW command and saves the goal only when the interrupting
    goal is neither follow nor hold.
  - Hold accepts hazard and alert and refuses board and crew wait; it saves
    as Follow does.
  - Board and UnBoard refuse board, crew wait and saving.
  - Trooper and Scout delegate to Patrol (state 2), Snipe (4) and Mine (5),
    and test the charge in state 3. Repair delegates to Patrol (4), Snipe
    (5) and Mine (6), tests the charge in state 2, and its target policy adds
    a 25 m horizontal limit.
  - Patrol accepts hazard and alert, keeps the base target and acoustic
    policies, and refuses board and crew wait.
  - Snipe (state 3, occupied) rejects shots known to come from its forward
    cone, takes targets only outside the cone and within 40 m, takes acoustic
    contacts outside the cone, and refuses board and crew wait.
  - Mine accepts hazard, alert and board and refuses the crew wait.
- **The charge test.** At most once per distinct time, a soldier counts
  allied soldiers within 7 (cap 30) whose controller is a UnitController in
  state 2, stopping at three. Exactly three plays the owner's ChargeSound
  (property hash `4ee58508`) and refuses interrupts 1 to 4 in the approach
  state.
- **Acoustic VO.** A successful priority 4 interrupt makes a soldier play
  HeardEnemySound (property hash `d3c54780`); droidekas and vehicles do
  nothing.
- **Priority 3's target** is the controller's deferred-LOS candidate, and a
  droideka refuses it beyond 10 m (squared distance over 100).
- **Common gate of priorities 1 to 4.** A class property (name hash
  `28adda49`) can mute the four priorities; stock data sets it only on
  vehicle classes (speeder bike, AT-TE, MTT, AT-ST...). Flyers inside a
  danger box padded by max(15, traversal radius) are muted too.
- **Squad commands from AI are chatter.** For commands 0 to 3 (move out, at
  ease, hold, follow), when the sender's controller is not a player's, only
  the VO plays: an AI "follow me" (Trooper state-1 entry, probability 0.3)
  and "move out" (Trooper arrival, probability 1.0) are heard, not obeyed.
  Board and unboard from AI drivers are delivered. The send probability is
  compared against a generator-B uniform (`<=`) and converts follow to board
  when the sender sits in a vehicle. Delivery is skipped while the game is
  paused.
- **Move out and at ease** (commands 0 and 1): a WaitSecondary agent's
  window becomes now − 2 (a Fly agent has its own side effect through the
  same `onMoveOut` hook); a follow or hold agent terminates when the
  controller's last handled command (stored after every handled command)
  came from the same Character. Hold reads that same record's issuer at its
  current controllable.
- **Hold refusal** is the optional `refusesHold` hook, for a recipient
  boarded in certain vehicle classes; the Vehicles binding never refuses.
- **UnitController construction.** The crew window is now + 4 for walker,
  hover and flyer owners, else 0; a stationary flag is set for stationary
  controllers; the owner's forward at creation is kept; the roll is
  A-integer(0, 100) (read by weapon AI and AttackPatterns).
- **Wait look.** ±75° (±30° stationary) about the home direction from
  generator A, 1000 out, rotated with the table sine/cosine (2048 samples of
  `sinf/cosf(i · 2π/2048)`, linear, truncated index); re-picked when
  A-integer(1, 9) is 2 each callback. A zero-speed owner with no target
  memory leaves after the stationary wait.
- **Cube vectors:** three uniforms, the first becoming z, then y, then x,
  each `2(u − 0.5)`.
- **Follow state 100.** Ending a follow clears the leader (no combat target
  is cleared). Clearing the leader also drops the executor and the trail.
- **Patrol revalidation.** The "valid objective" test is whether the post has
  a CaptureRegion, then the friendly-spawn test; it runs once, strictly after
  start + 5 s (sentinel 1e8).
- **Trooper objective retry.** Only when the post's center has moved more
  than 20 since the approach began (mobile posts), after 15 s, and the unit
  moves away from it or the helper arrived with status 0.
- **LOD and input parity bits** are separate: the scheduler's (toggled each
  scheduler update) and the simulation list's (toggled each phase-0
  traversal). The scheduler's cycle-reset call does nothing.

## Provisional

- `index.js`: `identity(unit)` for the parity gates is `unit.id` (the
  original uses the entity's identity snapshot, new per entity; pooled port
  units reuse ids). Skip patterns have not been compared with the original.
- `index.js`: controllers are scheduled in spawn order (the original inserts
  at creation; the insertion appears to append, unconfirmed).
- Without the Classes module the policy bases for Snipe/Mine cannot execute
  jobs. Normal module loading binds the implementations in
  `docs/ai-classes.md`.
- `agents.js`: Board/UnBoard/WaitSecondary vehicle bookkeeping goes through
  hooks; without the Vehicles binding boarding finishes at once.
- `agents.js`: an agent's `exit()` calls the helper's `terminate()` when Nav
  provides one, else `stop()` (the original terminates the helper's own FSM).
- `scheduler.js`: Combat's dive request becomes `intent.jump` with
  `intent.right = ±1`, `forward = 0` (the port's dive input).
- `scheduler.js`: a droideka's wanted mode becomes a `use` press when it is
  rolled (deploy) or standing (roll); the original's extra blocking flag is
  not modeled.
- `patrol.js`: an empty hint stance mask stands (the original would loop).
- `agents.js` (`UnitRandomAgent`): a soldier still within a metre of where
  its last timed retry (3 s) left it jumps as it sets off again (a jet
  request with the zero vector: an ordinary jump, a jet for a jet trooper).
  Classic recovery never jumps; a soldier in a hollow of steep slopes (Hoth)
  slid back from every direction it tried, for good.
- `patrol.js`: in state 2 (an unfriendly post), a bot whose walk ends
  outside the capture region walks on to its center, and if that ends
  outside too (a region on a deck above or below the route's end: Yavin's
  temple region is 36 m over the ground its routes reach), asks 20 s later
  for an objective other than this post (the exclude flag, as the 5 s
  revalidation does), or chooses about this post again if none is given.
  The offset spans the region's box unrotated, past a round region's edge,
  and state 2 has no timer: the bot took no part in the capture and stood
  beside the post for as long as it held.
- `controller.js`: a set latch over an emptied queue starts nothing (the
  original would start a stale record); the drain loops stop after 64
  transitions with a warning (the original has no bound).
- `controller.js`: an objective published while phase 1 starts goals (a
  mounted patrol asking for a post as it begins) starts at the next phase
  1, as publication does; the drain stops there. Going round again started
  it, and its start published again: 64 agents and objective draws a
  service, and the bot (a tauntaun on Hoth) never acted.
- `controller.js`: `unbind` resets the Nav control, so a dead bot's path
  search and queued request leave the planner at once.
- `scheduler.js`: no camera means no LOD skips.

## Unknowns left as hooks

- The name of the class flag behind the gate of priorities 1 to 4 (property
  hash `28adda49`).
- Vehicle-specific branches: mounted patrol (state 1 with a boarded
  vehicle), AT-AT stop in Trooper state 2, the low-ticket pilot exit
  predicate, hold refusal, mount reach, seats/reservations, danger boxes for
  flyers, vehicle notifications.
- Droid controllers (`UnitDroidController`, DroidRepairAgent, goal 7) and
  stationary/fly controllers belong to the Vehicles and class modules.
- Follow's path recorder and hold flags now use NavControl/PathExecutor;
  details and synthetic boundary tests are below.

## Follow path and mounted objective integration

`GroundGotoHelper` implements `setLeader`, `followTrail`, `holdPath` and
`installPath`. The state2 approach records the leader's subsequent movement
separately from its original approach path. Leaving state2 installs/appends
the recording; subsequent input steps append leader samples to that executor
without invoking the route planner. State4 holds advancement of its explicit
two-identical-point path and suspends leader sampling. Terminal leader clear
releases both paths. Agent destruction alone still does not clear the leader.

Sampling subtracts input dt, acts only below zero, draws generator B's
three-component cube offset in leader-local space (Y discarded), and appends
only for squared displacement strictly greater than 0.0625. A successful
append resets the timer to 0.25; an unsuccessful distance test leaves it
expired. Recording caps at 30 nodes; the continuing executor likewise trims
to 30 and resets its current node if trimming overtakes it. A completed
following path remains appendable until the leader reference is gone. Local
offsets use the soldier's horizontal forward basis; full tilted entity
transforms remain a limitation for custom non-soldier leaders. Follow
behavior has not been compared with full-game runs of the original.

`src/ai/index.js` now supplies Population's `crew(character)` and
`vehicleMask(vehicle)` callbacks from live seat leases. A driver skips
command posts whose bans match its AISizeType and republishes its selected
post to AI passengers; AI passengers copy the current AI driver's post.
Human occupants are never republished. Seat changes are read on each query.

`tests/ai-follow-integration.test.js` checks strict sampling thresholds,
RNG consumption, bounded trails, release of path pools, held advancement,
append after completion, generation cleanup, actual seat-to-Character
mapping, driver ban masks and objective propagation.
