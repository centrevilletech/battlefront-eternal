# Classic AI: perception, aim, firing and combat

Combat part of the Classic AI (`docs/ai.md`). It covers perception, targets,
memory and weapon eligibility; aim dynamics, physical spread and firing
decisions; range and pattern selection, pattern states and timers; the
droideka combat policy; lock-on, reload and zoom; targeted-fire and damage
alerts and agent alert acceptance; deferred target ranking, capacities and
tie closure; the cover helper's candidate and execution policy; melee attack
patterns (pattern 6); combat hazard movement; investigation versus reaction
timing; and mounted perception special cases. Further behavior details are
under "Further behavior details".

| File | What |
| --- | --- |
| `src/ai/classic/aim.js` | the direction filter and `Aimer`, skill curves, float32/D3DX vector helpers, lookup trig, projectile speed/gravity, ballistic and relative-motion lead, the aim-state point |
| `src/ai/classic/weapons-ai.js` | weapon AI data (`weaponAI`, ai.json registration), ranges, utility, weapon choice, `FireControl`, the run-length buttons and the fire-request consumer, fire validation, friendly-human protection, sniper yaw, the shared RNG for bot spread |
| `src/ai/classic/perception.js` | heaps, the sight scan, the deferred line-of-sight queue, thinning, the target tracker, hearing, grenade danger, targeted-shot and damage alerts, the interrupt responses |
| `src/ai/classic/combat.js` | the engagement dispatcher, `UnitCombatAgent`, `AttackPatterns`, `CoverHelper`, the binding the Controller's combat adapter takes (`classicBinding`) |
| `src/weapons.js` | additive AI options on `Weapon`/`Arsenal` (below) |
| `tools/zero_ai_classes.py` | `public/sides/<side>/ai.json` |
| `tests/ai-combat.test.js`, `tests/fixtures/ai-combat/` | tests; synthetic class/weapon fixture |

The modules import only `three`, `src/ai/random.js` (through siblings) and
each other; they run in Node.

## Where it runs

Per simulation step `h` (docs/ai.md "Tick order"), with the Controller's
`ClassicAI` and scheduler calling through its combat adapter
(`src/ai/classic/index.js combatAdapter`; `classicBinding(ai)` entries win):

1. `ai.update(h)`, decide:
   - `combat.ingest()`: the journal's `shot` and `damage` records become
     controller alerts (`notifyTargetedShot`, `notifyDamage` ->
     `controller.notifyShot(sourceCharacter, known, point)` on every
     controller `alertControllers(target)` finds: the unit's own, a seated
     soldier's seat control, and every AI crew control of a vehicle or a
     turret, as in the original game). A turret's damage reaches the journal
     through `Turret.onDamaged` (game.js), a round's with its damage, factors
     and the 30 m back-projection, so a gunner hears his turret being hit.
   - At a scheduler cycle wrap: `broadcastAcoustic(humanUnit, world)` for
     every human, and `rebuildProtected()` (protected humans).
   - For each serviced controller: phase 0 runs its agent. For a fighting bot
     that is `UnitCombatAgent.behave(dt)`: `tracker.select(world)` (take up
     the deferred-LOS candidate), `tracker.assignWeaponTarget(world)`
     (`setTarget`), the interrupt helper (which calls `hazardResponse`,
     `alertResponse`, `acquiredTarget`, `acousticResponse`), the state and
     pattern work, then `tracker.fireControls(world)` (requests A/B). Other
     agents call `updateTarget` (`tracker.update(world, dispatch)`). Then
     `DeferredLos.drain(controller)`, `scan(controller, world, dispatch)`,
     `broadcastAcoustic(unit, world)`, phase 1 (`thin()` into the queue),
     `clearCandidate(controller)`.
   - After the loop: `DeferredLos.service(world, budget)` (6, 12 on Hard,
     doubled when the planner is idle); a clear ray makes the controller's
     candidate (`recordTarget`).
2. `ai.update(h)`, execute (every AI unit, every step; a LOD skip carries
   its dt): `consumeFire(controller, unit, step, intent)` writes
   `intent.buttons`; `aimPoint(controller, unit, dt, out)` gives Nav's
   analogs the aim-state point (state 0: the target led by the AI dt).
3. `unit.tick(h)`: the soldier moves, then `unit.onMoved(h)` ->
   `ai.aim(unit, h)` -> `combat.aim(controller, h)`: the `Aimer` filters the
   body's aim into `intent.aim` (and hands the cannon in hand curves B and
   S); `combat.aimOverride = intent.aim`; the weapons fire along it with the
   bot spread rules. Until `aim()` is first called the binding runs the
   Aimer inside `consumeFire` instead (one tick earlier than the original
   game).
4. Ordnance: each live grenade or sticky round, every step:
   `combat.grenadeUpdate(round, position, ownerUnit, firstContactTime)`
   (see "Integration points").

## Public API

### aim.js

- `filterDirection(d, q, v, dt, A, D, out)`: one step of the direction
  filter (30-degree world-Y correction, acceleration clamp A, damping D,
  D3DX normalize), float32 operation for operation. `v` is updated in
  place.
- `class Aimer { direction, velocity, reset(dir), update(desired, dt,
  curves, weapon, hero) }`: per controller (`controller.perception.aimer`).
  Non-cannons halve A and D; heroes skip the filter; on cannons it sets
  `weapon.spreadScale = B`, `weapon.sniperScale = S`; `dt <= 0` is skipped.
- `skillCurves(skill)` -> `{A, D, B, S}` (anchors at skills 1, 14, 20;
  Population's `ClassicDifficulty` is the authority), `aimCurves(curves,
  weaponDef)`.
- `isCannon(def)` (cannon, weapon, launcher, grapplinghookweapon),
  `isLauncher(def)`.
- `projectileOf(def)` -> `{speed, gravity}`; `ballisticLead(p, v, origin,
  speed, gravity, out)` -> validity; `leadPoint(h, shooter, target, weapon,
  out)` (h seconds of relative motion, then the projectile solver);
  `dropCorrect(point, shooter, weapon, out)`.
- `aimPointFor(controller, unit, dt, out)`: aim state 1 the stored point,
  0 the led target (else 30 m along the aim), otherwise null.
- `lookupCos/lookupSin`, `ANGLE`, `dot32`, `lengthSq32`, `lengthSqXZ32`,
  `normalizeD3DX`, `normalizeOrX`, `normalizeXZ`.

### weapons-ai.js

- `weaponAI(def)`: the AI view of a weapon class record (or charge level):
  `{minRange, optimalRange, maxRange, implicitRange, extremeRange,
  relationMask, categoryMask, barrageMin, barrageMax, barrageDelay,
  triggerSingle, zoomMax, action}`, from `def.ai`, else the registered
  ai.json record of `def.class` (levels matched by `at`), else the record's
  own flattened keys (`MinRange`, `MaxRange`, `Target*`, `Barrage*`, ...)
  over the engine's WeaponClass defaults.
- `registerAIData(aiJson)`, `attachAIData(unitsJson | units, aiJson)`.
- Ranges: `effectiveRange(unit, controller, def)` (implicit MaxRange
  doubled for non-infantry owners; `controller.rangeScale`; the extreme cap),
  `expandedRange` (+5 s of nominal speed), `maxExpandedRange`,
  `sensorRange` (selected weapon's expanded range), `lowerBand`,
  `upperBand`. Facts: `forwardSpeed`, `categoryOf`, `relationBit`,
  `ammoCount`, `capacity`.
- `weaponUtility(owner, weapon, target)` -> `{ok, score}`;
  `selectWeapon(controller, unit, target, switchAllowed, world)` ->
  `{ok, index, weapon, action, inBand}` (in band first, capacity over 20,
  utility, earlier on ties; the grenade slot `roll % 10 ==
  trunc(trunc(time) / 3) % 10` or a target cluster flagged within 5 s).
- `class FireControl` (on `controller.perception.fire`): channel, the two
  run-length buttons, charge/sniper/burst state, the weapon's target,
  protected humans.
- `switchWeapon(controller, unit, index)` (uses the new
  `Arsenal.select`), `setTarget(controller, unit, target, world)` (masks
  gate the weapon's own target).
- `pressButton(button, down)`, `triggerFromCounts(previous, current)`,
  `arsenalInput(previousButtons, intent)` -> the `Arsenal.update()` input
  plus reload/next/zoom edges (`src/units.js IntentControls` is the
  in-game equivalent).
- `consumeFire(controller, unit, world)`: requests A (direct) and B
  (validated) shaped by charge, semiautomatic (with the sniper gate) or
  burst discipline, the protected-human veto, the sniper yaw; writes
  `unit.intent.buttons`; returns the request.
- `validateFire`, `facingAndRange`, `firingCone` (22.5 deg, 45 for melee;
  beyond 50 within 20 of the line; a zoomed single-shot soldier within 0.25),
  `rebuildProtectedHumans(world)`, `protectedVeto`, `sniperYaw(d2)`,
  `prepareSpread`, `useClassicRandom(arsenal, random)`, `chargeWeapon`,
  `sniperWeapon`, `selectedWeapon(unit, controller)`, `clusterState`.

### perception.js

- `class PriorityHeap(capacity, sentinel)`: `insert` (returns the
  displaced item, the newcomer when dropped, or null), `pop`, `peek`,
  `peekPriority`, `minIndex`, `slots`, `size`, `clear`.
- `class Perception` (`controller.perception`): local heap, `candidate`
  ref and `candidatePoint`, `aimer`, `fire`.
- `scan(controller, world, dispatch)`: radius clamp(maxExpandedRange, 75,
  350), first 175 hostile units, validator, collision category 16 skipped,
  77.5-degree cone, strict `L2 < (30 + 20 r)^2 m^2`, dispatch eligibility,
  priority `20000 - max(1, cost)` (+10000 cost for weapon-only targets).
  `world.vehicles.sharedTarget(controller)` replaces the broadphase for the
  human-driven mount case.
- `class DeferredLos` (`world.deferredLos`, one per match): `submit`,
  `drain(controller)`, `service(world, budget)` -> whether any test ran,
  `overflow`.
- `thin(controller, world, retainedRef, queue)`, `recordTarget`,
  `clearCandidate`.
- `class TargetTracker` (`agent.tracker`): `target`, `freshTarget`,
  `point`, `select(world)`, `assignWeaponTarget(world)`,
  `fireControls(world)`, `update(world, dispatch)`, `reset/assign`,
  `setDelay(s, now)`, `defaultDelay(now)`, `freeze(on, now)`.
- Hearing: `noiseRadius(unit, sounds)`, `broadcastAcoustic(source, world,
  sounds)`. Danger: `dangerDelayRange(skill)`, `notifyDanger(world, state,
  position, owner, enemySkill, onWarn)`. Alerts: `notifyTargetedShot(world,
  shooter, target)`, `notifyDamage(world, victim, attacker, packet, known,
  point)`, `alertControllers(target, out)`.
- Interrupt responses (the Controller's `InterruptHelper` calls them and
  builds the goal): `hazardResponse(controller, world, agent)`,
  `alertResponse(controller, world, agent, dispatch)`,
  `acousticResponse(controller, world, agent)` -> `{kind: 2, mode, point,
  exit, alert}` or null.
- Geometry: `matchupPriority`, `rawCost`, `coneDistanceSq`,
  `boundRadius`, `sightRange`, `validTarget`, `losPoint`, `clearShot`.

### combat.js

- `dispatch(controller, unit, target, switchAllowed, world)` ->
  `{pattern, probability, aux, action, inBand, weapon}`; `rangePolicy`,
  `roleOf(classDef)`.
- `class UnitCombatAgent extends Agent` (controller state 2): source goal
  mode 0/1/2/3 -> states 2 (investigate), 4 (fight), 5 (evade), 6 (prone); 3
  looks around. Policies: `acceptsHazard`, `acceptsAlert(known, point)`,
  `acceptsTarget` (always false), `acceptsAcoustic(source)`,
  `acceptsBoard(candidate)`, `acceptsRecovery` (true), `savesOnInterrupt`
  (false).
- `class AttackPatterns` (patterns 2-6, 8-12; 7 and 13-16 through
  `world.vehicles`), `class CoverHelper`, `findCover`, `coverScore`,
  `boardingClear`, `inForwardCone`.
- `classicBinding(ai)`: `createPerception`, `attachController` (shared RNG
  for the bot's weapons), `detachController`, `rebuildProtected`,
  `leadPoint`, `aimPoint`, `consumeFire`, `aim`, `updateAim`, `markSkip`,
  `ingest`, `grenadeUpdate`. It also registers `world.classData`'s ai.json
  records.

## What other code provides

- `world.units` (`query`, `relation`, `living`), `world.los(from, to, a, b)`,
  `world.time`, `world.random` (A), `world.randomB` (B), `world.difficulty`
  (`curves(team)`, `enemySkill`, `hard`, `dangerRow()`), `world.events`.
- Units: `kind`, `team`, `hero`, `human`, `alive`, `generation`,
  `position`, `velocity`, `sphere`, `aimPoint(out)`, `origin(out)`,
  `forward(out)`, `soldier` (`posture`, `air`, `jet`, `fuel`), `classDef`,
  `combat` (`arsenal`, `maxHealth`), `intent`, `controller`, `character`.
- Controller (see `ai-controller.md`): `unit`, `ai`, `world`, `generation`,
  `roll`, `rangeScale`, `perception`, `fireA`, `fireB`, `target`,
  `aimState/aimPoint/aimTarget`, `lookAt`, `aimAt`, `lookAlongPath`,
  `setStance`, `requestFacing`, `requestJet`, `wantedMode`, `dodge`,
  `alertAnim`, `shot`, `danger`, `acoustic`, `notifyShot`, `notifyDanger`,
  `addAcoustic`, `difficulty` (curves).
- Movement helper (Nav): `goto(point, options)`, `stop()`, `tick(dt)`,
  `arrived`, `status`. Cover hints: `world.nav.hints.query(4, center, half,
  30)` (or `ai.nav.queryHints`), hint `position`, `forward`,
  `primaryMask`, `secondaryMask`, `sideMask`, `siblings`. Optional Nav hooks:
  `world.nav.jetHints.{find, excluded, enter, tick, exit}` (pattern 11),
  `world.nav.directClear(unit, point)` (melee), `world.nav.segmentBlocked(a,
  b, unit)` (direct jets), `world.nav.inDangerRegion(unit)` (flyers).
- Vehicles, on `world.vehicles`: `dispatch(controller, unit, target,
  switchAllowed, world, out)` for walker/flyer/hover sources,
  `enterPattern(patterns, state)`, `tickPattern(patterns, state, dt)`,
  `exitPattern(patterns, state)` for 7 and 13-16, `sidestep(patterns)` from
  pattern 2, `sharedTarget(controller)`; and on `ai.vehicles`:
  `mountReach(unit, point)`, `currentControllable(character)`.

### Integration points (outside Combat)

What the host provides:

- Grenade danger: every step, for each live grenade/sticky round,
  `combat.grenadeUpdate(round, round.position, ownerUnit, round.contactTime
  ?? Infinity)`, with `contactTime` set to the simulation time at the
  round's first touch (`ordnance.onGrenadeContact`). The `grenade` journal
  record alone cannot drive it (the notifier draws a fresh delay every
  update and re-warns at most every second while the round lies there).
- `shot` records from the human: `target` (the player's weapon or lock
  target, when there is one). Bot shots fall back to the shooter's weapon
  target.
- `damage` records: `amount` as the packet's base damage, `typed` (the
  health-type scale) and `generic` (the shield scale) for the "more than 1
  by either" test, and `known: true, point` for directional hits (a bolt:
  30 units back along its flight).
- `world.vehicles` hooks (above), with `sidestep`'s per-second rate bound by
  whoever assembles them.
- Optional `createClassicAI(world, { weaponSounds: (weapon) => muteDistance
  })` for per-weapon fire-sound mute distances.
- Audio: `vo` records with `property: "FleeSound"` (the class's
  FleeSound, e.g. the stock `*_inf_com_chatter_flee`) at the record's
  position.

## Weapon options (`src/weapons.js`)

All additive; a player's weapons behave as before.

1. Header comment describing the AI options.
2. `new Weapon(def, { player = true, random = null })`: `random`, a
   `() => [0, 1)` draw, is kept on the weapon.
3. New fields `random`, `spreadScale = 1`, `sniperScale = 0`,
   `yawSpreadOverride = null`.
4. `spread()`: the yaw term is `yawSpreadOverride ?? YawSpread`.
5. `shoot()`: with `random` set, each round draws its pitch pair, then its
   yaw pair, from it (the original game's order); otherwise `Math.random` as
   before.
6. The per-salvo kick adds `SpreadPerShot * spreadScale`.
7. `Arsenal.select(weapon)`: select a weapon in its channel (refused while
   the channel's weapon is busy).

`{ player: false }` (existing) keeps the kick from dropping below zero.

## ai.json (`public/sides/<side>/ai.json`)

`python3 tools/zero_ai_classes.py rep cis` (or `--archive x.lvl --out
y.json` for one archive). Reads `SIDE/<side>.lvl` plus the shared
`common.lvl` and `core.lvl`; every `entc`/`wpnc` class the side defines.

```js
{
  schema: 1, generator: "tools/zero_ai_classes.py 1", side,
  source: { path, bytes, sha256 }, shared: [{ path, bytes, sha256 }],
  weapons: { <class>: {
    class, base, archive,                       // archive: "side" | shared file
    ai: { minRange, optimalRange, maxRange, implicitRange,   // MaxRange authored -> false
          extremeRange, lockOnRange,            // extreme = larger of ExtremeRange/LockOnRange as read
          relationMask, categoryMask,           // TargetAll then Target* bits, in authored order
          barrageMin, barrageMax, barrageDelay, zoomMin, zoomMax (0 -> 1),
          triggerSingle, maxPressedTime?, animationBank?, lockTime?, clearLockOnFire?,
          canDeflect?, action,                  // AnimationBank -> AI action (0 pistol, 1 rifle,
                                                //   2 grenade, 3 launcher, 4 melee, 6 tool)
          engineClass, cannon },                // ODF label -> WeaponCannon/..., cannon family
    explicit: { <Key>: "<last raw value>" },    // what was authored
    defaulted: ["MinRange", ...],               // left to the class defaults
    levels?: [{ at, ai, explicit }],            // NextCharge levels, their own replay
    ordnance?: { class, base, engineClass, Velocity, Gravity?, Damage, *Scale,
                 explosions: { ExplosionImpact|ExplosionExpire: {...} } },
    raw: [[hashHex, name | null, value], ...],  // every property, archive order
  } },
  classes: { <class>: {
    class, base,
    ai: { unitType, role (0 scout, 1 anti-armor, 2 assault, 3 support/repair/special,
          4 pilot, 5 trooper, 6 other), sizeType, sizeMask, vehicleType,
          maxHealth?, addHealth?, maxSpeed?, ..., capturePosts?, noEnterVehicles?,
          healthType?, pilotType?, fleeSound?, chargeSound?,
          tactical?: { ai: {AISC*Sound}, human: {SC*Sound}, acknowledge: {...} },
          loadout: [{ weapon, ammo, channel }], sections?: [...] },
    explicit, defaulted, raw,
  } },
}
```

The runtime reads `weapons[*].ai` (and levels) through `registerAIData`
(`world.classData`, done by the binding) or `attachAIData`.

## Further behavior details

- Two generators: every combat draw uses generator A except the cube
  offsets for the unengageable-alert dodge point (mode 2) and the
  direct-jet offsets (pattern 10), which draw from B.
- `UnitCombatAgent` policies beyond the alert and hazard rows:
  `acceptsTarget` (the scanner's target) is always false; `acceptsBoard`
  only when no valid hostile is within 8 of the candidate (cap 30), then the
  base board policy; `acceptsSecondaryWait` (the crew wait) false;
  `acceptsCommand` true; `acceptsRecovery` true; `savesOnInterrupt` false.
  Accepting the scanner's target would restart the agent each perception
  cycle, so that answer matters.
- Weapon class tests follow the label-to-engine-class registration: the
  cannon family (aim halving off, B/S setters live) is cannon, weapon,
  launcher and grapplinghookweapon.
- Projectile speed/gravity as the lead reads them: bolts, bullets, fat
  rays, tow cables at Velocity with no gravity; missiles 3e8 (no lead);
  shells, grenades, sticky, haywire and beacon ordnance fall at Gravity x
  -9.8; beams and emitters 0.
- The D3DX normalize the filter uses is single precision: a reciprocal
  square root refined by one Newton step, zero below 2^-46. The filter, the
  lead solver and the skill curves match the original game bit for bit.
- A soldier's sight bound radius is its posture's collision radius:
  standing 1.4626346 (sight range 59.2526932), crouched 1.1022704, prone
  1.6220657; in the air it counts as standing.
- Cost and sight distances run from the eye (`origin`) to the target's aim
  point; the line-of-sight point is midway between the target's eye and aim
  point.
- Thinning (more than four candidates) draws `int(1, 2)` for each pop while
  another candidate remains, the retained target included (it is drawn for
  but never skipped).
- The cluster scan (grenade preference) runs at most every 5 s per target:
  radius 9 around it, the first 10 non-neutral units, abort on any ally of
  the attacker, three soldiers (the target included) flag it for 5 s.
- A shot's spread draws its pitch pair before its yaw pair.

## Fire pace

How often a bot's weapon fires, by weapon family, in the 2004 game, and
what the port measures (`tools/fire-pace-probe.mjs`, whole battles
headless; `tests/ai-fire-pace.test.js`, the chain end to end). The port's
pace is the original's, family by family (numbers below).

The clock. Offline the original runs one simulation update per rendered
frame, with that frame's delta (deltas clamp at 0.1 s; only the video
option's vsync setting limits the rate). Each update runs the decision
scheduler, then the thread list, where every AI soldier's input thread
(enabled for any controller that is not a player's) executes its input
before the soldier itself and its weapons. The one skip is the distance and
visibility gate: a bot not within 45 m of camera 0, not fast and outside
every view runs every other update, its weapons too, the skipped delta
carried (the port skips the input only and updates the weapons every
update, to the same pace). The port runs the same order at its fixed 60
updates a second, which is the original at 60 frames a second.

The requests. A bot's decision (every C updates, C = 15, 3 on Hard) clears
its two fire requests and writes them again: B while its target is fresh,
else A when the remembered point is within 3 units of the target, in the
cone and in clear view. They stand until the next decision. The consumer
shapes them every update into the channel's run-length button, which the
weapon reads directly.

By family, at 60 updates a second (a bot near the player or in view; "far"
is a bot the gate runs every other update):

- Single shot (TriggerSingle, no charge, ZoomMax under 6): the consumer
  holds the button two updates and releases two, and the weapon fires on a
  button count of exactly +1, so there is a press every 4 updates and a
  round every ShotDelay rounded up to that grid: 0.2 s is 12 updates (5 a
  second), far 16 (3.75). Heat ends the string: a stock pistol (0.175 a
  round, 0.3 a second back, threshold 0.25) fires 9 rounds (far 10), then 7
  (far 8) per string, silent about 2.9 s between; 1.8 rounds a second while
  the request stands. A ShotDelay under 4 updates fires no faster than the
  presses: the jet trooper's commando pistol (0.05 s) fires every 4 updates
  (15 a second, far 7.5), 6 to 8 rounds, then 2 s.
- Automatic (a cannon without TriggerSingle): the consumer fires bursts of
  BarrageMin..BarrageMax rounds (the engine's 3 and 6; no stock weapon sets
  them) at ShotDelay, then waits BarrageDelay (2 s) from the burst's end; a
  burst starts only if its heat fits under 1 - heat. A request that drops
  for one update (the target not seen again by the decision, a validation
  failing) restarts the count with no wait. A rifle: 1.7 rounds a second. A
  bottomless weapon's count never moves (its ammunition reads INT_MAX), so
  heat ends its bursts: the droideka's repeater fires 26 rounds at 0.11 s,
  then 2 s (5.6 a second).
- Zoomed single shot (ZoomMax 6 and up): one round per uniform(2, 3) s with
  the weapon idle.
- Charging (MaxPressedTime above 0, launchers excepted): a hold of 0.25..1
  of the weapon timer's limit, ShotDelay after a shot, so a bowcaster bot
  fires single bolts; falling rounds wait 4 s after a release.
- Launchers take the single-shot path and never wait for a lock.
- Vehicle and turret guns: the vehicle patterns request directly; cannons
  burst as above.

Difficulty adds no fire discipline: none of the code that shapes fire reads
it. It sets the decision cycle and the line-of-sight budget (how soon
targets are taken up), the aim filter's A and D and the spread curves B and
S.

The frame rate. The press cycle is 4 frames in the original, so at 60
frames a second it is the port's. Single-shot weapons whose ShotDelay is at
least 4 frames fire alike from 15 frames a second up (a pistol 0.2 to 0.27
s); only shorter ones follow the frame rate (the commando pistol, 7.5
rounds a second near at 30 frames, 15 at 60). Bursts, delays and heat are
in seconds.

Measured, Medium, 90 s, seeds 1 and 2 (`node tools/fire-pace-probe.mjs
--maps nab2,tat2,geo1,kas2,bes2,hot1 --seeds 1,2 --seconds 90`): 4.5 to 18.7
bot rounds a second per battle. A gap is between one shooter's fire cycles,
a burst is cycles under 0.5 s apart (a dropped request restarts a burst at
once, so two can run together), and the rate is per second of a standing
request:

| Weapons | Fire cycles | Rate | Median gap | Mean burst (longest) | The original's |
| --- | ---: | ---: | --- | --- | --- |
| Pistols | 2466 | 1.44 | 0.2 to 0.267 s | 5.3 (11) | 0.2 s near, 0.267 far; 9 or 10, then 7 or 8 a string; 1.8 a second |
| Commando pistol | 275 | 3.07 | 0.133 s | 6.5 (10) | 0.067 s near, 0.133 far; 6 to 10, then 2 s |
| Rifles, wrist blasters | 6417 | 1.64 | 0.2 s | 3.6 (10) | 3 to 6 at 0.2 s, then 2 s; 1.7 a second |
| Droideka repeater | 2057 | 5.18 | 0.117 s | 15.3 (29) | 26 at 0.11 s, then 2 s; 5.6 a second |
| Sniper rifles | 406 | 0.49 | 2.3 to 4.2 s | 1 | one per 2 to 3 s |
| Launchers | 257 | 0.43 | 2 to 4.1 s | 1 | ShotDelay and reloads |
| Shotguns | 140 | 0.45 | 0.8 to 1.9 s | 1 | ShotDelay 0.75 s on the press grid: 0.8 s |
| Bowcasters | 175 | 0.78 | 0.57 to 3.2 s | 1.1 | single bolts |
| Gun turrets | 331 | | 0.4 s | 5.9 (10) | bursts at ShotDelay |
| Vehicle guns | 412 | | 0.1 to 4.5 s | 2.7 (9) | bursts at ShotDelay |

What is heard and seen follows the shots. Every soldier's fire cycle plays
FireSound, and a new one lets the last play out, as `src/audio/weapons.js`
does: a pistol bot near the player is five sounds a second. Bolts fly at
Velocity. The laser draws a core of min(LaserLength, flown) half LaserWidth
wide and a glow of min(GlowLength, flown) 1.6 LaserWidth wide; the port's
Classic look draws those lengths, and its Enhanced look a 1.8 m capsule for
the infantry blasters, shorter than their 3 m glow (measured side-on at
10 m: rifle 2.3 m Classic, 1.9 m Enhanced). Neither look draws a shot
longer or faster than the original's; a class that names no GlowLength
(vehicle and turret guns, the commando pistol) draws shorter in the port,
without the glow the original gives it (`docs/formats.md`, "Bolts").

One difference, left alone: the port's trigger edge (`IntentControls`,
`arsenalInput`) takes a press from a never-pressed button (count 0 to +2) as
a single-shot press, where the original wants exactly +1; there a bot's
first single shot of its life comes three updates later. It changes no
pace.

## Provisional

- `perception.js noiseRadius`: walkers, hovers and droidekas use 2 x speed
  (cap 50) until the sound table is wired; the original reads their
  engine/event MuteDistances (x5 for walkers and droidekas). Flyers are the
  class constant 1 regardless of their engine-sound state.
- `perception.js weaponNoise`: weapon states 1..5 are read as "charging,
  busy or in its shot delay"; the second sound states 0/8 read for some
  weapons is not modeled; MuteDistance comes from `options.weaponSounds`,
  else `def.MuteDistance`, else 400 (the stock infantry fire sounds).
- `aim.js projectileOfWeapon`: while charging, the lead uses the reached
  level's ordnance; the original interpolates along the charge chain.
- `weapons-ai.js clusterState`: kept on the target's controller, or the
  unit for humans (the original keeps it on every controller).
- `weapons-ai.js rebuildProtectedHumans`: protection goes to the
  controllers of units within 50 of the human; the original adds every AI
  seat of a nearby vehicle.
- `aim.js Aimer.update`: skips `dt <= 0` (the original would poison its
  rate with NaN) and restarts on non-finite input. Safe-port divergence.
- Mounted aim is filtered before the seat input batch; on-foot aim runs after
  soldier movement and before firing. Full parity with the original's
  coupled clock is not claimed by the browser battle checks.
- `weapons-ai.js weaponUtility` tests the weapon's category mask only for a
  soldier's soldier target; other targets go by the damage rules alone, as
  in the original. A wider mask test would leave the Hoth turrets' gunners
  (their cannons clear every category bit) without a weapon for anything,
  so they would never take a target or fire. `validateFire` compares the
  target with the controllable's own target, not with the weapon's masked
  target: SetTarget writes the controllable's target whatever the masks say,
  so a torpedo launcher (TargetBuilding 0) picked against a turret fires
  unguided instead of never firing. The masks still keep the weapon's own
  target clear, and that target is what a launcher's lock follows. The shot
  alert's fallback target (`ingestEvent`) is likewise the controllable's
  channel target, not the weapon's.
- `weapons-ai.js burstRequest` (port): a burst whose heat (salvos x heat
  per salvo) exactly fills the gauge counts as one above it (0.99), so it
  starts once the heat is all but gone. The original multiplies and
  compares in single precision, and heat cools to exactly 0: such a burst
  is neither above 1 nor under 1 - heat, so it never starts, and while the
  target stays in view the request never drops to redraw the count. Without
  the port's rule, a Hoth turret's cannon (0.25 a round) would stall whenever
  it drew 4 rounds (one burst in four): its gunner would aim at his attacker
  and never fire again. A speeder bike's 5 rounds of 0.2 (1 in single
  precision) would stop the same way.
- `combat.js AttackPatterns` (pattern 9's exit): the cover helper's own exit
  runs before its state is zeroed. Its states 2 and 3 hold the target
  (`TargetTracker.freeze`), and only that exit lets go; skipped, a bot that
  left cover for another pattern (a new dispatch, another target) kept a
  target it no longer saw for the rest of the fight. Pattern 2 has no
  lost-target step, so it stood facing where the target was last seen (a
  sniper on Tatooine, 117 s, until the target died).

## Gaps (left as hooks)

- Vehicle dispatch, patterns, shared human target and mounted reach are now
  bound through `world.vehicles` / `ai.vehicles`; see `ai-vehicles.md` for
  their scoped fidelity limits.
- Jet hints (pattern 11) now use Nav's authored hint service; see `ai-classes.md`.
- Pattern 8 is implemented but dormant (the dispatcher never assigns it).
- No other teammate-intelligence sharing is known.
- Hero fear or morale: none in Classic.

## Tests

`node --test tests/ai-combat.test.js`: curves at every skill and tier
(Population's difficulty when present), non-cannon halving, the filter and
its 30-degree branches, the Aimer, lookup cosine, the lead solver, spread
buildup per salvo, the AI draw order and the player's untouched path, every
sniper yaw step and gate, button run-lengths and the semiautomatic
two-and-two, the sniper uniform(2, 3) gate on the shared generator, bursts,
charging, ranges and selection, utility, masks, the firing cone, protected
humans, both heaps, the deferred-LOS queue, priority and cost with the
+10000 penalty, the sight cone and range, thinning, clusters, the tracker,
acoustic broadcast, danger delays per tier and the binding's grenade step,
targeted shots, the range policy table, dispatch, the agent's policies, the
extractor on a synthetic archive, and two squads fighting through the whole
Classic AI step. Checks against optional local reference data skip when it
is absent.

`node --test tests/ai-fire-pace.test.js`: the fire pace end to end at 60
updates a second, a standing request through the consumer, the trigger
edges and the weapon: single shots every 12 updates near and 16 far until
the heat ends the string, no faster than every 4 updates whatever the
ShotDelay, rifle bursts of BarrageMin..Max with BarrageDelay between, and a
bottomless heat weapon's burst ending on its heat.
