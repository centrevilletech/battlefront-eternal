# Classic AI: controller, scheduler, agents, interrupts, patrol, squads

Runtime ownership is described in [Native AI ownership](sim-ai.md). Bot policy
runs in `rust/sim/rust/ai/` (ported from JavaScript). Function names are the
Rust names where the port keeps one, usually the JavaScript name in
snake_case; object-shaped examples document the behavioral contract.


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
| `rust/sim/rust/ai/control/controller.rs` | the controller's data (source goals, `SourceGoal`, `CommandQueue`, request/alert fields), `goal`/`state`/`role`/`combat_mode` |
| `rust/sim/rust/ai/control/fsm.rs` | the generic state-machine state every controller, agent and helper shares |
| `rust/sim/rust/ai/control/agents.rs` | agent data (`AgentKind`: Wait, WaitSecondary, Random, Follow, Hold, Board, Unboard, the objective agents Trooper/Scout/Repair, MissingCombat), the interrupt helper's state, the degraded direct helper |
| `rust/sim/rust/ai/control/policy.rs` | interrupt acceptance policies, the charge test |
| `rust/sim/rust/ai/control/patrol.rs` | patrol data, patrol-hint query and choice, command-post geometry |
| `rust/sim/rust/ai/control/squads.rs` | squad command kinds, the VO bank and entry lookup |
| `rust/sim/rust/ai/control/scheduler.rs` | the candidate budget, the camera gate, the degraded analog conversion |
| `rust/sim/rust/ai/control/mod.rs` | `Core`: binding, the per-service update, every state machine's entries, interrupts priorities 1 to 8, squad delivery, the scheduler and the AI input step |
| `rust/sim/rust/ai/control/world.rs` | `AiWorld`, what the core asks of the rest of the battle |
| `rust/sim/rust/ai/owned/runtime.rs` | the battle's AI entry points over the canonical stores |
| `rust/sim/tests/ai_control_twin.rs`, `replay_ai.rs`, `rust/sim/rust/ai/owned/tests/phase.rs`, `rust/sim/rust/ai/owned/tests/native_phase.rs` | tests |

## Using it

The native owner (`rust/sim/rust/ai/owned/`) drives one `Core` per battle
through `Runtime` (`owned/runtime.rs`):

- `attach(character)` for every roster Character (humans are ignored);
- `spawned(character)` once a Character's unit exists, `died(character)`
  when it dies;
- `phase(dt)` once per simulation step, before units tick (it runs
  `Core::update`);
- `Core::player_command(character, kind)` for the local player's squad
  buttons (the page calls `playerCommand` on its AI object).

`update(h)` = `decide(h)` then `execute(h)`:

1. `decide`: Combat's event ingestion (shots/damage become controller alerts),
   spawn/death sync for anything not reported, then the scheduler: cycle step,
   budgeted controller services (phase 0, deferred-LOS cleanup, sight scan,
   acoustic broadcast, phase 1, candidate clear), then the path-planner and
   deferred-LOS budgets.
2. `execute`: the AI input step for every live AI unit, filling `unit.intent`.

`spawned` and `died` are optional (`update` notices both), but they keep the
original game's RNG order: it builds a new controller per spawned entity
(drawing its roll) before Population republishes the objective. Native
spawning calls `spawned` at the point `docs/ai-population.md` describes.

### World fields read

The core reads the battle through `AiWorld` (`control/world.rs`), which
`owned/world.rs` implements over the canonical stores:

| Fact | Use |
| --- | --- |
| the simulation clock | every timer (seconds) |
| generator A | the AI's draws (see Further behavior details) |
| generator B | the game's own draws (squad rolls, recovery, jet retries) |
| units and relations | ally queries, recruitment, weak references |
| the roster | follower counts, delivery, humans for the acoustic cycle |
| difficulty | the cycle and line-of-sight budget (`control/difficulty.rs`) |
| navigation | movement helpers, controls, planner, barriers, hints (`nav/mod.rs` `Nav`) |
| player cameras | the LOD gate only (view matrix, near, far, fov, aspect, zoom) |
| the event journal | VO records (`kind: "vo"`) |
| mission controls | `formation`, `stayInTurrets`, `stationaryWait` |
| objectives | Population's objective requests (`Core::objectives_request`) |
| LOD disabled, network, paused | gates |

## Module APIs

### `control/controller.rs`

```text
Controller::new(character)
Core::bind(ci, unit) / unbind(ci)          // per life (attach, spawned, died call these)
Core::controller_update(ci, dt, phase)     // phase 0 or 1 (the scheduler calls this)
state, pending, agent                      // controller FSM: 1..11
commands                                   // CommandQueue: current, queue (<= 5), latch, current done
Core::publish_objective(ci, post)          // source goal 3, replacing the queue (Population calls this)
Core::request_objective(ci, exclude)       // Population's request
// request fields (cleared each phase 0 where the original clears them)
request                                    // MoveRequest (Nav's control replaces it)
aim_state, aim_point, aim_target           // 0 aim_target (+lead), 1 aim_point, 2 along the path
target, fire_a, fire_b                     // Combat's firing target and requests
facing, stance, range_scale, jet_pending/jet_vector, wanted_mode, exit_request, dodge
look_at(p) / aim_at(ref) / look_along_path() / set_stance(s) / request_facing() / request_jet(v)
Core::request_exit(ci)
leader                                     // follow leader ref (what follower counts read)
roll                                       // integer 0..100 drawn per life (Combat reads it)
nav_control, difficulty                    // perception lives in Combat's CombatAi
// alert intake (Combat's producers); cleared after each phase 0
notify_danger(point)
notify_shot(source, known, point)
add_acoustic(ref)                          // at most 20
add_vehicle_note(ref)                      // at most 10
receive_command(squad_command)
danger_* / shot_* / acoustic / vehicle_notes / command_record / last_command
executor_status(), has_executor(), avoid_side()   // read through the nav control
```

`SourceGoal` records: `{ kind, refA, refB, post, point, mode }`;
constructors `objective(post)`, `combat(mode, { point, target })`,
`follow(leaderRef, sourceRef)`, `board(vehicleRef)`, `droidRepair(t, o)`,
`new(kind)`. `CommandQueue`: `overwrite`, `append`, `prepend_one`,
`prepend_two(saved, goal)`, `pop_front`, `remove_kind`, `needs_next_goal`,
`current_copy`. The state machine (`fsm.rs`, driven by `Core::fsm_update`,
`fsm_flush` and `fsm_terminate`): `state`, `pending`, `Core::request_state`,
`done`, with each owner's enter, exit and tick.

### `control/agents.rs`: the agent base (UnitAgent)

```text
Agent { kind: AgentKind, fsm, goal, helper, interrupts, ... }   // built on state entry
Core::fsm_update(Owner::Agent(ci), dt, phase == 0)   // returns state == 100
Core::exit_agent(ci)            // controller state exit: helper terminate (or stop), then terminate
Core::agent_on_enter / agent_on_exit   // the agent's own state entry/exit
Core::agent_behave(ci, dt)      // phase-0 per-state work; the base tick calls it in phase 0
                                //   and Combat's rank_targets in phase 1
// shared steps
Core::update_combat_target(ci)  // Combat's full tracker update
Core::aim(ci, stance, look)     // posture request, then aim at the fresh target, else the
                                //   remembered point, else `look`, else along the path
Core::helper_tick, can_engage(ci, unit), board_policy(ci, unit)
// policies (Core, with policy.rs)
accepts_hazard()                // grenade/sticky danger
accepts_alert(known, point)     // targeted shot / damage
accepts_target(unit), accepts_acoustic(unit), accepts_board(unit)
accepts_secondary_wait(), accepts_command(kind), accepts_recovery(), saves_on_interrupt(goal)
on_move_out()                   // move out / at ease side effect (crew wait, flyer)
```

Agents (`AgentKind`): Wait (1), the objective agent with its Trooper, Scout
or Repair flavor (3, by UnitType role; `ObjectiveFlavor`), WaitSecondary
(4), Board (5), Unboard (6), Random (9), Follow (10), Hold (11). State 2 is
Combat's `CombatAgent` (`MissingCombat`, which finishes at once, stands in
when combat is not bound).

The movement helper an agent drives (Nav's ground helper, `nav/goto.rs`;
the core reaches it through `Core::helper_goto`, `helper_stop`,
`helper_tick` and the rest):
goto a point with mode route (default), direct (the patrol's roaming) or
straight (the recovery's two-point path); stop, tick, arrived, status,
`helper_destination`, `helper_set_leader`, `helper_follow_trail`,
`helper_hold_path`, `helper_install_path`, `helper_terminate`.

### Interrupts (`control/mod.rs`, `control/policy.rs`)

`Core::interrupts_tick` runs priorities 1..8 and returns
whether one fired. When Combat's `hazardResponse`, `alertResponse` and
`acousticResponse` are bound they decide priorities 1, 2 and 4 (perception
math is Combat's); the helper still applies the shared gate, builds the
goal, and handles their `exit` (turret leaves) and `alert` (HeardEnemySound)
flags. Fallback implementations are used otherwise.
`Core::interrupts_charging` is the charge test.

### `control/patrol.rs`

`Patrol` (the helper's data; the core drives its states 1 to 6 and 100):
`look`, `look_point`, `stance`, `hint`, `expires`. Helpers:
`capture_region`, `cp_center`, `cp_offset`, `arrived_at_objective`,
`query_patrol_hints`, `choose_hint`, `hint_available`, `pick_hint_stance`,
`friendly_spawn_eligible`; a used hint is marked through Nav's `mark_hint`.

Hint fields read: the hint kind, position, forward, its command post, the
primary stance bits (`primary_stance`; default stand), the reservation time
(default −10) and the radius siblings, through Nav's hint query
(`nav/hints.rs`).

### `control/squads.rs`

`command` (0 move out, 1 at ease, 2 hold, 3 follow, 4 board, 5 unboard),
`SquadCommand` (`controller.rs`: source, kind, time, target),
`Core::send_command`, `distribute_command`, `player_command`,
`count_followers`, `current_controllable`, `vo_entry(kind, context, human)`,
`vo_offset`, `VO_BANK`, `VO_PROPERTIES`; VO goes out through the world's
`emit_vo`.

VO records go to `world.events` as `kind: "vo"`: `{ event, property, source,
team, position, human, ... }`. Events: `tac_moveout`, `tac_hold`,
`tac_follow`, `tac_needgunner`, `tac_spreadout`, `tac_stop`, `tac_allclear`
(command VO, with `command`, `context`, `positional`, `notice` for the local
player's HUD); `tac_yes` (acknowledgement, AISCResponseYessirSound);
`heardEnemy` (HeardEnemySound); `charge` (ChargeSound). Audio maps
`property` through the unit's class.

### `control/scheduler.rs`

`candidate_budget(N, C, t)`, `camera_accepts(camera, point)`,
`fallback_analogs(...)`, `visible_or_near(unit, cameras, ...)`, and the
`Scheduler` state (`stats`, the cycle counter, the parities, the skips);
`Core::scheduler_numbers` gives the cycle and line-of-sight budget, and
`Core::update`, `decide` and `execute` run the services and the input
step.

The input step per unit: the skip gate (identity parity against
`inputParity`, not visible, not skipped last time; a skipped step's dt is
added to the next), then Combat's `consume_fire`, `intent.exit` for an exit request, then
Nav's `navigation_input` (the executor step, analogs, jet, posture), then
the droideka's wanted mode as a `use` press and Combat's `controller.dodge`
as a sideways jump. The aim point is Combat's `aim_point` (none for "along
the path").

### Services the core calls

The core reaches every sibling through `AiWorld` (`control/world.rs`); a
service a battle does not bind is a no-op, as the capability flags in the
setup say. The native owner binds them all:

- Nav (`nav/mod.rs`, `nav/goto.rs`, `nav/units.rs`): the movement helpers,
  `navigation_input`, `analogs` (fallback), the planner's idle and service
  steps, barrier tests, `traversal_radius`/`traversal_mask`, hints.
- Combat (`combat/`): perception, the target tracker, `scan`, the one
  `DeferredLos`, acoustic broadcast, candidate clearing, phase-1 thinning,
  sight and sensor ranges, the selected weapon, `consume_fire`, the aim
  point, the three responses, `CombatAgent`, and the rest of `CombatAi`
  (`create_perception`, `attach_controller`, `ingest`, `aim`, `lead_point`,
  ...).
- Population (`control/goals.rs`, `control/difficulty.rs`,
  `rust/sim/rust/battle/spawning.rs`): objective requests, friendly-spawn
  eligibility, `Core::curves_for`, `Core::is_hard`, post AI properties,
  objective centre, offset and arrival.
- Vehicles (`vehicles/service.rs`, [ai-vehicles.md](ai-vehicles.md)): the
  board, unboard and wait-secondary agents and the vehicle services
  (boarded vehicle, current controllable, seats and reservations, low
  tickets, pilot seat keeping, alert blocking, mount reach, hold refusal,
  target reservation, mounted patrol, AT-AT test, notification, exit
  requests, mounted helpers).
- Class behaviors (`control/classes.rs` and the core,
  [ai-classes.md](ai-classes.md)): the snipe and mine helpers, snipe and
  mine selection, repair job search and the repair service.

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
  target reservations); 4 acoustic refs in order → combat at the initial
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
  helpers live in `control/classes.rs` and the core; see [ai-classes.md](ai-classes.md).
- Patrol: states 1 to 6 and 100, driven by the core (`control/mod.rs`);
  data and geometry in `control/patrol.rs`.
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
- Squads: `Core::send_command` and `distribute_command`. Acceptance: the recipient's count
  (followers + 1) < 2 and the leader's < 3.
- Scheduler: `control/scheduler.rs` and `Core::update`.

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
  same `on_move_out` hook); a follow or hold agent terminates when the
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
- **Cube vectors:** three uniforms, the leading value becoming z, then y, then x,
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

- `control/mod.rs`: the identity for the parity gates is the unit's id (the
  original uses the entity's identity snapshot, new per entity; pooled port
  units reuse ids). Skip patterns have not been compared with the original.
- `control/mod.rs`: controllers are scheduled in spawn order (the original inserts
  at creation; the insertion appears to append, unconfirmed).
- Without the classes capability the policy bases for Snipe/Mine cannot
  execute jobs. The native owner binds the implementations in
  `docs/ai-classes.md`.
- `control/mod.rs`: Board/UnBoard/WaitSecondary vehicle bookkeeping goes through
  hooks; without the vehicles capability boarding finishes at once.
- `control/mod.rs`: an agent's exit calls the helper's terminate when Nav
  provides one, else stop (the original terminates the helper's own FSM).
- `control/mod.rs` input step: Combat's dive request becomes `intent.jump` with
  `intent.right = ±1`, `forward = 0` (the port's dive input).
- `control/mod.rs` input step: a droideka's wanted mode becomes a `use` press when it is
  rolled (deploy) or standing (roll); the original's extra blocking flag is
  not modeled.
- `control/patrol.rs`: an empty hint stance mask stands (the original would loop).
- `control/mod.rs` (the Random agent): a soldier still within a metre of where
  its last timed retry (3 s) left it jumps as it sets off again (a jet
  request with the zero vector: an ordinary jump, a jet for a jet trooper).
  Classic recovery never jumps; a soldier in a hollow of steep slopes (Hoth)
  slid back from every direction it tried, for good.
- Patrol: in state 2 (an unfriendly post), a bot whose walk ends
  outside the capture region walks on to its center, and if that ends
  outside too (a region on a deck above or below the route's end: Yavin's
  temple region is 36 m over the ground its routes reach), asks 20 s later
  for an objective other than this post (the exclude flag, as the 5 s
  revalidation does), or chooses about this post again if none is given.
  The offset spans the region's box unrotated, past a round region's edge,
  and state 2 has no timer: the bot took no part in the capture and stood
  beside the post for as long as it held.
- `control/mod.rs`: a set latch over an emptied queue starts nothing (the
  original would start a stale record); the drain loops stop after 64
  transitions with a warning (the original has no bound).
- `control/mod.rs`: an objective published while phase 1 starts goals (a
  mounted patrol asking for a post as it begins) starts at the next phase
  1, as publication does; the drain stops there. Going round again started
  it, and its start published again: 64 agents and objective draws a
  service, and the bot (a tauntaun on Hoth) never acted.
- `control/mod.rs`: `unbind` resets the Nav control, so a dead bot's path
  search and queued request leave the planner at once.
- Scheduler: no camera means no LOD skips.

## Unknowns left as hooks

- The name of the class flag behind the gate of priorities 1 to 4 (property
  hash `28adda49`).
- Vehicle-specific branches: mounted patrol (state 1 with a boarded
  vehicle), AT-AT stop in Trooper state 2, the low-ticket pilot exit
  predicate, hold refusal, mount reach, seats/reservations, danger boxes for
  flyers, vehicle notifications.
- Droid controllers (the droid repair controller and agent, goal 7) and
  stationary/fly controllers belong to the Vehicles and class modules.
- Follow's path recorder and hold flags now use NavControl/PathExecutor;
  details and synthetic boundary tests are below.

## Follow path and mounted objective integration

The ground helper (`nav/goto.rs`) implements leader following, the follow
trail, path holding and path installation (`Core::helper_set_leader`,
`helper_follow_trail`, `helper_hold_path`, `helper_install_path`). The state2 approach records the leader's subsequent movement
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

The world's `crew(character)` and `objective_vehicle_mask(vehicle)`
(`control/world.rs`, served by `owned/crew.rs`) read live seat leases. A driver skips
command posts whose bans match its AISizeType and republishes its selected
post to AI passengers; AI passengers copy the current AI driver's post.
Human occupants are never republished. Seat changes are read on each query.

`rust/sim/rust/ai/owned/tests/phase.rs` checks follow handles across recycled actors
and previous lives, and following humans; the per-phase replays
(`rust/sim/tests/replay_ai.rs`) cover sampling, trails and objective propagation
against captured battles when a capture corpus is supplied.
