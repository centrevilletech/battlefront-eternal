# Classic AI class jobs

`src/ai/classic/classes.js` supplies the real `SnipeHelper` and `MineHelper`
through the existing `helpers` export, and pilot/support services through
`classicBinding(ai)`. `loadClassicModules()` loads it automatically. The
existing Trooper, Scout and Repair agents keep their state dispatch, Scout
60 to 100 second deadline, five-second pilot job scan and interrupt policies.

`src/ai/classic/nav/jet-hints.js` is installed as `ClassicNav.jetHints`.
Combat's pattern 11 already calls `find`, `enter`, `tick` and `exit`.

## Behavior

- Sniping: mode and attacker relationship filtering, 100-unit box/cap20,
  float priority heap, at most eleven available suitability attempts,
  low-ticket gate, configured forward sphere/cap175 and strict cylinder
  approach danger. No visibility ray is invented. Approach marks the hint;
  occupation revalidates every15 seconds, uses the strict normalized 3D
  facing threshold, selects the authored stance and writes range scale2.
  External selection retries after 8 to 12 seconds; failed approach immediately
  searches again. Reservation checks and marks use Nav's four-second gate
  and linked siblings.
- Mines: finite/infinite ammunition and MaxItems availability, infinite-life
  mine dispenser recognition, hint search every10 seconds, 3m XZ lattice,
  strict interior radius, cap30 allied broadphase objects followed by mine
  filtering, spacing at least2.25m, ordinary inventory selection followed by
  one fire request and a strictly-over-three-second placement wait. A failed
  arrival postpones the hint reservation timestamp by30 seconds. No mines
  or ammunition are created by the helper.
- Pilot service: 75-unit/cap50 candidates, finite health-type capabilities,
  dead/unbuilt structure support, low-ticket veto, nearest stationary target
  selection excluding ordinary AI soldiers, and priority for humans within
  30m using the original literal `cos(15 radians)` test. Health/ammo choice
  draws only when both apply. Approach uses the strict20-second no-path/stuck
  guard; work uses the original health-progress guard after8 seconds for
  either job kind. Tools hold the trigger; dispensers request one throw per
  progress interval. Selection, aiming and firing go through shared weapons.
- Jet hints: 40-unit AABB/cap30, reservation filtering, random permutation
  with horizontal-distance scores and strict tie replacement. The helper
  approaches the origin, requests destination `position + 80 * forward`,
  and submits the authored vector literally, including non-unit vectors.
  Completion requires strictly more than1 second and a cleared Nav jetting
  flag. Combat owns retry timers; Soldier owns fuel and physical activation.

## World adapters

Normal units, weapons, mission controls and hints use the interfaces in
`docs/ai.md`. Additional inputs:

| Field | Contract |
| --- | --- |
| `world.items` | Shared Items service (`items`, `owned`), or an array of item records. Items provide position, owner, team, def, gone and radius. |
| `world.serviceCandidates(center, radius)` | Optional array of registry units and stable building/vehicle wrappers, including dead rebuildable structures. Filtering and cap50 happen in Classes. Without it only living registry units are scanned. |
| service candidate | position, generation, team, alive, kind, health/maxHealth (or combat equivalents), radius/sphere, healthType, optional human/humanPiloted, velocity, forward, combat.arsenal. |
| `healthType` | Numeric0 person,1 animal,2 droid,3 vehicle,4 building,5 rebuild,6 build; classDef.HealthType strings work for ordinary types. |
| `candidate.building` / `.vehicle` | Shared gameplay object, for comparison with `combat.repairTarget`. Wrappers must retain identity and expose live health/team fields. |
| `world.mineGroundHeight(point)` | Optional terrain height query when the map permits terrain snapping; mine aim is height+1.5. Omit for maps without that predicate. |
| `world.classCandidates(center, radius, cap, owner, relation)` | Optional unified broadphase for the first-hostile-object mine veto. Relation−1 means hostile. |
| `world.jetHintExcluded(unit)` | Optional map/world-specific authored-jet exclusion predicate. |

Supply stable generation refs and ordinary `origin(out)`, `aimPoint(out)`
and `forward(out)` methods on candidates used by combat aiming. AI requests
never apply repair quantities themselves: bot PlayerCombat must receive the
same `live` and `items` services as the player and use the bot's team.

## Additional behavior details

- The next outer snipe attempt is scheduled for now+8+uniform(0,4) seconds.
- The next outer mine attempt is scheduled for now+10 seconds.
- Minefield hint gathering uses half-extent 100 and cap 3, then accepts the
  first valid placement. Hints whose assigned command post does not admit
  friendly spawning are removed, swapping with the last candidate.
  Unassigned hints survive.
- The mine veto checks one hostile broadphase object inside hintRadius+6 and
  vetoes only when that first object is a mine. This is separate from the
  allied spacing check.
- The 30 second postponement writes now+30 into the same reservation
  timestamp; generic availability subsequently adds four seconds.
- Repair-tool fire requests repeat, but dispenser fire is requested only
  when initializing the progress timestamp.
- Service gathering takes at most 10 objects within 15 m and forms health/ammo
  masks from powerup items. Their team is not tested. A matching nearby item
  ends active service for that resource; the pickup itself remains gameplay.

## Contract changes

The three ordinary role agents now preserve MineHelper's placement-facing
request while its state3 is active. Otherwise their `lookAlongPath()` call
would overwrite it after the helper tick. No controller dispatch change is
needed for the supported soldier jobs. Repair aims at a point and approaches
with `goto` for dead build/rebuild targets because ordinary combat/navigation
weak-reference resolvers intentionally reject dead objects.

## Pilot service: port changes against standing pilots

Found with `tools/ai-stuck-probe.mjs` (docs/ai.md "Stuck bots"); pilots
were the largest group of bots that stopped moving, and their planning
churn the main cause of everyone else's. Tests: `tests/ai-stuck.test.js`.

- A wreck's walk is planned once (`approach`): the service called `goto`
  every update, each one restarting the plan and stopping the bot until the
  path came, so the path was always thrown away before it was used. On
  Cloud City that was over a third of all planner work.
- "Locked on" needs a lock: with none, the empty `current` matched the
  service wrapper's missing `building`/`vehicle` (`undefined === undefined`),
  every job was "close" inside the tool's default 60 m range, and the pilot
  stood that far off cutting at nothing.
- Close by the 5 m rule but not locked yet, a repair tool closes in, once,
  in a straight line to the target's bounds plus 1.5 m (`closeIn`): the
  planning barriers around a turret keep a route's end outside the cutter's
  reach (its lock range past the target's bounds, from the eye).
- A tool that never comes to bear (the cone never passes, so no work and
  no progress check) gives up after 8 s.
- A job given up (20 s approach gate or 8 s progress check) stays off that
  life's list for 30 s (`futile`): the next five-second search otherwise
  picked the nearest job again, the same one, without end.

## Remaining integration limits

- Custom self-piloted repair-droid source goal7 and its25-second work/owner
  death lifecycle still require controller/dispenser support. The ordinary
  controller deliberately has no state8. Stock remote recon must not be
  assigned that repair goal, and this module does not deploy recon or call
  orbital strikes.
- Without `classCandidates`, the first-hostile-object mine veto checks the
  living unit registry before items; the original combined entity insertion
  order can differ. Without unified service candidates, buildings cannot be
  selected. These are explicit adapter limits, not alternate tactics.
- Service target reservations are local to this class binding with the
  shared three-second generation-checked lifetime. They are not yet backed
  by a common building/vehicle reservation manager.
- Float distances are float32 at heap insertion; the surrounding geometry
  uses Three.js numbers. Class-job behavior has not been compared
  bit-for-bit against the original game. Synthetic fixtures cover decision
  boundaries and shared weapon requests; they do not establish full-game
  behavioral parity.

## Validation

`timeout 300 node --max-old-space-size=1024 --test tests/ai-classes.test.js`
covers configured sniping geometry and mode gates, eleven attempts,
occupation/revalidation, mine inventory/caps/grid/allies/spacing/fire wait,
failed approach cooldown, pilot human preference, ammo-only health guard,
approach timeout, dead-build weakrefs, jet query RNG and authored impulses.
The controller and Nav suites exercise the existing dispatch and movement
interfaces alongside these helpers.
Two pipeline fixtures run the real signed fire-control adapter and Arsenal:
one consumes a secondary-channel mine round, the other calls the real
PlayerCombat repair tick and LiveObjects repair method to add exactly the
fusion cutter's health quantity, using the bot's own team.
