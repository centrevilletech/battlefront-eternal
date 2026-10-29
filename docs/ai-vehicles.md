# Classic AI: vehicles, turrets and seats

Vehicles, turrets and seats for the Classic AI (`docs/ai.md`). Two layers:

- **Seats** (gameplay, `src/seats.js`, `src/vehicles.js`, `src/gunner.js`,
  turret parts of `src/props.js`): who sits in which vehicle seat or turret,
  the player and bots alike; how a seated bot's intent drives its seat; exits;
  destruction; the vehicles and turrets as perception targets.
- **Classic vehicle AI** (`src/ai/classic/vehicles/`): boarding and leaving,
  the vehicle notifier, the mounted controllers and their agents, the vehicle
  families' AI input conversion, vehicle attack patterns, flyer objectives and
  landing, unattended-vehicle decay.

Behavior follows the 2004 game; "Findings" records the rules worked out while
implementing.

## Seats

### Leases

Every vehicle seat (`vehicle.seats[i]`) and every turret holds at most one
**lease** (`SeatLease`, `src/seats.js`):

```js
lease = {
  unit,          // the occupant: a Unit, or the player's unit (null in bare tests)
  generation,    // unit.generation when the lease was granted
  human,         // the local player's lease
  team,          // the occupant's team at boarding
  vehicle | turret, index,   // where
  role,          // "pilot" (seat 0) | "gunner" (a seat with weapons) | "passenger"
  since, tick,   // vehicle-system time and input-batch count at boarding
  input,         // SeatInput: this update's frozen intent (bots)
  controllable,  // SeatUnit: the seat as the AI drives it (bots), below
  exitRequest,   // { post, promote, placement } until the next batch takes it
  live,          // getter: not released, and (bots) same life of its unit, alive, still seated here
  origin(out), forward(out),   // the seat's eye (a turret's gun pivot) and aim direction
}
```

The lease **is** the unit's seat reference: while a unit sits,
`unit.seat === lease` (so `unit.seat.vehicle`/`.turret`/`.index` as the
contract names them). A seated unit is also `unit.targetable = false`
(perception targets the vehicle; Combat's `validTarget` honors the flag) and
`unit.combat.mounted = true` (its own weapons wait).

Boarding, leaving, a death and the vehicle's destruction clear leases at once.
A lease whose unit died or respawned since (generation changed, `alive` false,
or `unit.seat` no longer this lease) is **stale**: it steers nothing, and the
next input batch lets it go without moving the body.

### One input batch per update

`Vehicles.update(dt, driven, input)` keeps its signature and the player's path
(`driven`/`input`) unchanged. Per call, before the 120 Hz substeps, every live
bot lease freezes its intent into `lease.input` (`SeatInput.latch`):

- held values copied: `forward, right, turn, pitch` (clamped to [-1, 1]),
  `brake`, `up`, `down`, `aim` (normalized `intent.aim`, the Classic aimer's
  direction) and the trigger holds (`buttons[c] > 0`);
- edges taken from the intent once: a trigger press is a count going from
  not-held to held since the **lease's** previous batch (its own history, not
  the unit's `IntentControls`); `reload`, `use`/`exit` (the exit request) are
  read and cleared; `jump, crouch, prone, nextPrimary, nextSecondary, zoom`
  are dropped, so they do not fire later on foot.

Every substep of that update sees the same values; a moved bot's edges are
dropped (`Vehicles.move`), so changing seats mid-update cannot replay one.
The intent read is the seat controllable's when a controller drives it
(`lease.intent`), else the occupant's own (a soldier-bound controller, or a
test).

Order inside `Vehicles.update`: spawns; latch (stale leases released, exits
carried out); substeps (seat 0's batch drives a bot-piloted hull, the player's
`input` his hull as before; every bot seat's aim turns by its analogs); render;
bot seats fire; tethers; bot bodies placed at their seats.

### Driving and gunning

- **Hull**: seat 0's occupant steers it: the player through `VehicleDriver` as
  before, or a bot through its batch, using the same `Vehicle.tick` code (turn
  rate, strafe, brake, a flyer's up/down and nose pitch).
- **Aim**: every seat has its own aim (`seat.aimYaw`, `seat.pitch`). The
  player's look moves the aim of the seat he is in (`vehicle.aimYaw`, `pitch`,
  `lookYaw`, `lookPitch` are accessors for `seats[seatIndex]`, so the camera,
  HUD and tests read as before). A bot's turn and pitch analogs turn its seat's
  aim at the seat's `TurnRate`/`PitchRate` (seat 0's turn is the hull's
  steering); seat 0's aim is what the hull follows. Both are held inside the
  seat's limits (`Vehicle.clampAim`; docs/formats.md "Vehicle seats").
- **Weapons**: each manned seat's aimers turn toward its aim target
  (`Vehicle.aimSeats`): the point under the player's crosshair, or the first
  thing along a bot's `intent.aim` (else the seat's aim) from the seat's eye.
  Rounds leave the weapon's real fire points, at that point once a barrel
  points there, else along the barrel; a weapon with no aimer nodes fires a
  bot's rounds along its aim. The player's rounds are owned by the seated
  player Unit, with a hull fallback in unitless fixtures; a bot's are **owned
  by the bot** (kill credit, team), still ignore the hull's
  own body, and pass the firing barrel's beam emitter (below). Unoccupied seats are
  untouched, as before. A bot's seat weapons use the AI spread floor
  (`Weapon { player: false }`), the player's the player floor (provisional).
  Taking a seat resets the shared weapon's spread multipliers and random
  source before its new controller binds. A human taking over an AI seat
  therefore does not inherit the AI's difficulty curves or random stream.
- **Presentation**: a bot's body sits at the seat's rider node and takes the
  seat's pilot pose, visible only on a seat with a pilot animation (an exposed
  rider), exactly as the player's (`poseRider`, shared with
  `VehicleDriver.sync`). Shown riders need `vehicles.presentRiders(dt)` after
  units are posed, to take their node's place and turn (or the saddle, on an
  animal); the player's keeps `driver.presentRider`. Both keep the body
  hidden on an enclosed seat every frame, as a remote soldier record (or, for
  the player's own body, each correction from the world) shows it again.
- **Origin**: a seat's `origin` (the controller position) is its eye,
  `EyePointOffset` in the seat's frame.

### The player and bots together

Only when bots are aboard does anything differ for the player:

- Entering: the seat he last used if free, else (as the original game seats a
  player) the first seat that is free or held by a bot; that bot moves to the
  next free seat after its own, or gets out (no promotion).
- F (next seat): a bot in the next seat trades places.
- Getting out voluntarily hands his seat to the last bot aboard (promotion).
- The player's unit holds his lease (`playerUnit.seat`, `targetable = false`)
  while he drives or mans a turret, so soldier raycasts and perception pass his
  body by as they do seated bots.

### Exits

`vehicles.unboard(unit, { policy, force, placement, promote, post })`:

- `policy: "classic"` (bots, default): the original game's seat exit
  placement (`classicSeatExit`): nine candidates in the occupied seat's frame
  at R = bounding radius + 0.684 (behind, left, right, the four 0.7071 R
  diagonals, in front, centre), 1 m up; the first eight need a clear line from
  the vehicle's bounding centre (the vehicle left out); a candidate needs one
  of five world-axis support samples (±0.72) to find a floor (5 m up, then 10 m
  down); the X/Z of the candidate at the highest support; the vehicle's origin
  when nothing qualifies. No water, drowning, fall or death-region test (the
  Classic behavior). Exit velocity zero; jet fuel kept. Always succeeds.
- `policy: "safe"`: the player's own search (clear floor, room, a clear line
  out), refusing when boxed in unless `force`.
- `placement: false`: out without moving the body.
- `promote` (default: unless the unit's controller is in state 5, boarding):
  the last bot aboard, in seat order, moves into the vacated seat.
- `post`: carried to `onUnboard` (the original game assigns the saved post).

`vehicles.requestExit(unit, opts)` is the exit request: taken at the next
batch. An intent's `use` or `exit` edge does the same.

### Destruction

`Vehicle.setHealth` reaching 0 first lets go of every bot lease (all at once),
then kills those bots (`combat.damage(Infinity, ..., attacker)`, credited to
`vehicle.lastAttacker`), then runs the player's `onDestroyed` handler as
before. A turret's destruction (`Building.destroyed`) kills a bot gunner first
the same way (`TurretSeats.destroyed`). `Vehicle.damage(amount, attacker)` and
`Building.damage(amount, attacker)` take an optional attacker;
`Vehicles.struck/blast` and `LiveObjects.struck/blast` pass an optional owner.

### Turrets

`TurretSeats` (`src/seats.js`) mans props.js turrets with bots; the player
keeps the `Gunner` adapter. `man(unit, turret)`, `leave(unit, opts)`,
`requestExit(unit, opts)`, `update(dt)` (each frame, after
`LiveObjects.update`), `seatOf(unit)`, `canMan(unit, turret)`,
`attachUnits(units)`. A manned turret has `turret.gunner` (the body, as the
existing checks expect) and `turret.lease`. The bot's turn/pitch analogs turn
the drum at `MaxTurnSpeed` (as the mouse rates do), the gun follows what lies
under the bot's aim at the gun's own rates, and the trigger fires the turret's
weapon from its fire point along the gun, owned by the bot. Exits use the
Classic placement in the drum's frame. A gunner killed at the gun leaves it
at once (`src/boarding.js` `leaveKilled`, `TurretSeats.vacate` with reason
`killed`), his body falling from the seat; a shown one meets rounds and
blasts there (docs/live-objects.md "Turret gunners").

The drum's frame is the building's (`Turret.facing`, the instance's
heading), and the drum is posed along the aim from it whatever its bind turn
(docs/live-objects.md "Turret views and aim"), so Tatooine's barge
turret, whose drum is bound half round, points its gun where its view and
its bot aim. `Turret.steer` holds the aim inside `YawLimits` of that heading
(the barge turret's 70 either way; the stock turrets name the full circle),
the drum standing still at the arc's end.

The gunner hears his turret: a shot aimed at it or damage to it (a round's,
from 30 m back along it; a blast's with no known point) alerts his seat
control as the original game's alerts walk an armed building's controllables
(docs/ai-combat.md "Where it runs"). With known points, a gunner already
fighting turns to a new attacker only from outside his forward cone, and
one who cannot bring the gun to bear (`mountReach`: PitchLimits, YawLimits)
gets out unless the mission keeps bots in turrets.

`tools/turret-ai-probe.mjs` measures it in whole headless battles (per
turret: seconds manned, exits by reason, the barrel's angle to the nearest
enemy in view, targets, shots, hits, damage taken, the share of attacks
answered, bots standing by a turret) and in scripted duels (a bot seated,
the player shooting the turret from a chosen bearing and distance).

Bot turret rounds use the weapon's seeded yaw/pitch spread. Turret beams
follow the gun frame through an emitter owned by that lease, which expires
when the gunner leaves; human turret shots carry the player's unit for damage
and kill credit. Taking and leaving the human turret also updates
`combat.mounted`. Destroyed turrets without a wreck model disable their live
body, and rebuilding increments the registry target's generation so old
target references cannot resolve to the new turret life.

#### Showing the player's turret (`src/gunner.js`)

The world owns the drum's `yaw`, `pitch`, `yawVel`, `pitchVel` and the gun's
`gunYaw`/`gunPitch` (`Turret.steer`/`pose` in `src/props.js`, stepped by
`Gunner.update`). A page showing another world's battle (a world worker's, or
a server's) never steps them: they arrive with the turret's state, and the
page's seat adapter sets `gunner.turret` (`EntitySeats.setGunner`,
`src/runtime/entity-seats.js`) without `enter()`. The page draws from that
state:

- `Gunner.updateCamera(dt, zoom, firstPerson)` makes the views for whichever
  turret is set (`viewsFor`, a `TurretCamera` each), poses the drum and gun
  it shows (`poseTurret`, `src/seats.js`, which calls `Turret.present`: the
  drum along the shown aim, the gun at its own angles, never stepping them)
  and sets the camera. The views are the turret seat's own, as the original
  game's (docs/live-objects.md "Turret views and aim"): the chase camera
  rests at TrackCenter in the aim frame (the drum's pivot, looking along the
  aim) plus TrackOffset in its own, pitched down by TiltValue, 48 degrees;
  first person (the perspective key, `setPerspective`) looks from
  EyePointOffset in the aim frame straight down the aim. An eye inside the
  turret's box (the Hoth, dish and Geonosian turrets) puts the drum and gun
  meshes on the hidden layer, as his own body is in first person; third
  person puts them back, as does leaving.
- `Gunner.preview(pending, looks, scale, invert, alpha)` runs the drum ahead
  over the commands the world has not taken yet and the looks not yet in a
  command (`previewSteer`, the turret's own `steer`, a tick per command),
  then back through the tick shown by `alpha`, into `previewYaw`/
  `previewPitch`, which `updateCamera`, `viewDirection(out)` (the reticle's
  direction as shown) and the drum pose use. The page calls it each frame
  beside the soldier's look preview (`game.js` clientFrame); a page stepping
  its own world has nothing pending and may skip it.
- `TurretSeats.present()` poses the bots' turrets the same way, once a frame
  after the ticks, for a page that never runs `drive()`.

`tests/gunner-preview.test.js` covers the pure parts of the turret view and
preview.

### Registry targets

`vehicles.attachUnits(units)` (or the `units` constructor option) enters every
hull, now and later, in the units registry; `turretSeats.attachUnits(units)`
every turret. Entries (`vehicle.aiTarget`, `turret.aiTarget`):

| field | vehicle (`VehicleTarget`) | turret (`TurretTarget`) |
| --- | --- | --- |
| `kind` | 2 walker (creatures, command walkers), 5 flyer, 6 hover (the Hailfire too) | 8 armed building |
| `category` | 3 vehicle (1 for creatures) | 4 building |
| `team` | the first occupant's (pilot first), else the hull's own | the gunner's, else the building's (0) |
| `alive`, `dead`, `combat {health, maxHealth, dead, team, explode()}` | the hull's | built or not |
| `sphere {center, radius}`, `aimPoint(out)` | collision bounds centre, half the box diagonal | the building sphere |
| `origin(out)`, `forward(out)` | seat 0's eye and aim | the gun pivot, the drum's view |
| `position`, `velocity`, `classDef`, `className`, `generation` (bumped at each respawn) | | |

No `hitSpheres` (rounds strike them through collision bodies; the registry's
soldier raycast skips them). `combat.explode` is a no-op (blasts reach
vehicles through `Vehicles.blast`).

### Seat controllables (`SeatUnit`)

The original game gives every occupied control its own controller (a vehicle's
pilot seat, a gunner's or passenger's slot, a turret). `lease.controllable` is
that control as a unit (docs/ai.md interface), for a bot's controller to drive:

- `kind`: the pilot seat classifies as the vehicle (2/5/6); any other seat and
  a turret as 7 (mounted turret / passenger slot);
- `classDef`: aircraft speeds and rates come from `flight/config.js`; ground
  classes retain their authored values/defaults (walker MaxSpeed 20,
  MaxTurnSpeed 2, MaxYawSpeed 1,
  MaxPitchSpeed 0.5; hover ForwardSpeed, StrafeSpeed, TurnRate, PitchRate,
  MovingTurnOnly), `AISizeType`, `bounds`; a non-pilot control has zero speeds;
- `soldier`: a proxy (position, velocity, aimYaw/aimPitch of the seat,
  maxSpeed = nominal speed, maxStrafe, maxTurn, maxPitch, posture "stand");
- `combat.arsenal`: the seat's weapons (channel c = weapon c);
- its own `intent`, `seat` (the lease), `targetable: false`, `navRadius`,
  `origin/forward/aimPoint/sphere`, `bodyYaw`, `vehicle`/`turret`, `index`,
  `occupant`, `character`, `team`, `alive` (false once the lease moves or ends).

A bot moved to another seat gets a new one.

## Classic vehicle AI (`src/ai/classic/vehicles/`)

| module | contents |
| --- | --- |
| `index.js` | `classicBinding(ai)` → `VehicleAI` (all services), `agents`, `adaptClassicAI(ai)`, `StationaryHelper` |
| `agents.js` | `UnitBoardAgent` (state 5), `UnitUnBoardAgent` (6), `UnitFlyAgent` (7) |
| `controllers.js` | the mounted controller policies, `controllerPolicy(unit)` |
| `notify.js` | `VehicleNotifier` |
| `reservations.js` | `Reservations`, `freeCapacity` |
| `steering.js` | the vehicle families' AI input (`seatAnalogs`), shared flight virtual controller, walker yield |
| `patterns.js` | vehicle dispatch rows, patterns 7 and 13 to 16, pattern 2's sidestep |
| `fly.js` | `FlyGotoHelper` (Nav's, plus external paths and the landing request) |
| `service.js` | the per-AI service registry |

### Services

`VehicleAI` implements every name in the Controller's `VEHICLE_SERVICES`:

| service | behavior |
| --- | --- |
| `notify()` | the notifier, at the scheduler's cycle wrap |
| `createHelper(agent)` | a `StationaryHelper` for kind-7 controls, the flyer `FlyGotoHelper` for flyer pilots (with Nav), else null (Nav's helpers) |
| `requestExit(controller, post)` | the control's exit request; `post` becomes the Character's objective |
| `boarded`, `boardedVehicle`, `boardedVehicleOf(unit)`, `boardedVehicleOfCharacter`, `currentControllable(character)`, `isMountedTurret`, `emptySeats`, `crewControllers`, `seatsFull`, `pendingReservations` | seat queries (vehicles answered as their registry entries) |
| `lowTickets(team)` | teams 1/2 at 25 reinforcements or fewer |
| `pilotKeepsSeat` | false: every stock control has PilotType 2 (Findings) |
| `alertsBlocked`, `refusesHold` | false (no vehicle rule known) |
| `mountReach(unit, point)` | angular reach only, inclusive, body frame, YawLimits/PitchLimits |
| `canEnter(controller, candidate)` | the soldier's eligibility (Findings); a hull through `Vehicles.eligibleSeat`, the rules every boarder follows (src/boarding.js, docs/vehicles.md) |
| `freeSeats(candidate, team, type)`, `reserve(candidate, unit, type)`, `reserveSeat`, `holdReservation` | capacity and reservations |
| `boardPolicy(agent, candidate)` | the base boarding acceptance: a candidate that moves always; a stationary one (a turret) only with a remembered target it could fire at, or, with StayInTurrets, for any role but the repair roles |
| `boardTick` | the Controller's stub agent's tick (unused once `agents.board` is bound) |
| `targetReserved(target, team)`, `reserveTarget(target, unit)` | flyer target reservations, type 2 (Findings) |
| `patrolMounted(patrol)` | Patrol's mounted branch |
| `isAtAt(unit)` | the label/class the original game compares with "AT-AT" |

Additional services (for the hooks below and the vehicle agents):
`controllerPolicy(unit)`, `analogs(unit, request, aim, dt, out, options)`,
`boardNow`, `boardPathClear`, `holdHint`, `closeHint`,
`disembarkPassengers`, `destroyStuck`, `abandonPad`, `updateDecay(dt)`,
`sharedTarget`, `unreachableFromMount`, `walkerContacts`, and Combat's
`world.vehicles` hooks: `dispatch`, `enterPattern`, `tickPattern`,
`exitPattern`, `sidestep`.

### The notifier

At each controller-cycle wrap (Findings): with G AI cycles per second (30 /
cycle length: 2 Easy/Medium, 10 Hard) there are N = 3G buckets and a
persistent counter (N at first) visiting 0..N, so one pass in N + 1 is empty.
A boardable (built turrets, then vehicles) whose registry id mod N is the
counter is offered when not every seat is taken, slower than 3 m/s, (a
vehicle) where soldiers can reach it (planning barriers at 2 m and its own
radius), to the units within the notify radius (50; mission
`SetAIVehicleNotifyRadius`), at most 30, friendly or neutral to it, that an AI
controller drives, whose class may enter vehicles, of the vehicle's own team
when it has one (any team when a player drives it), and whose team is not low
on tickets. A told controller appends the entry to `vehicleNotes`.

### Boarding (state 5) and leaving (state 6)

The boarding interrupt (priority 6) is the Controller's; its gates call
`canEnter`, `freeSeats`, the agent's `boardPolicy`, the reboard time, and
reserve. `UnitBoardAgent` (Findings): entry (2) keeps or makes the seat
reservation and walks to the vehicle; each tick (after interrupts and the target
update, so it fights on the way) it gives up unless it may still enter, its
reservation holds (renewed) and the vehicle is all but still (speed² < 4);
within the two bounding radii plus 0.5 it boards (3), which seats it at once
when a seat is still free. It refuses danger, alerts, scanner targets and
sounds while the vehicle is uncontested (no eligible enemy within 8 m),
never takes boarding or the crew wait, and saves nothing.
`UnitUnBoardAgent`: its entry (state 1) requests the exit; it keeps its target
updated, refuses danger, alerts, sounds, boarding and the crew wait (scanner
targets, commands and recovery go through the base policies) and saves nothing.

### Mounted controllers

`controllerPolicy(unit)` for a seat control:

- **UnitMobileController** (hover and walker pilots): the soldier mapping
  (1..6 same, 8→9, 9→10, 10→11).
- **UnitStationaryController** (gunner/passenger seats, turrets): 1 and 3 →
  Wait, 2 → Combat, 6 → Unboard, nothing else; its Wait looks ±30° when the
  control belongs to a building (`controller.homeFacing`).
- **UnitFlyController** (flyer pilots): 2 → Combat, 3 → 7 (UnitFlyAgent),
  4 → crew wait, 10 → Hold (11), nothing else (so a flyer pilot takes no
  Unboard goal).

The Controller's own `UnitWaitAgent` already carries the stationary idle exit
(no target memory for the stationary wait: 10 s, 10000 s with StayInTurrets).

### UnitFlyAgent (state 7)

Policies in Findings. 1: set the flag; flying → 4, on the ground wait (→ 3).
2: a straight 200 m path ahead (1 m up, flags on) until 100 m straight up is
clear → 4. 3: a transport with its flag waits 10 s, anything else until the
literal time 4; then 4 if clear above, else 2. 4: up to ten random posts
(until one passes the ownership test or lies 100 m away), 20 m above it; every
3 s one landing hint within an 80 m cube (linked post, the flag's ownership
filter for transports, not above the flyer) → 5. 5: hold the hint; approach
L − 2·MaxSpeed·d to the landing point L = hint frame ×
(0, ½·LandingTime·LandingSpeed + LandedHeight, −½·LandingTime·MinSpeed), d =
hint forward with y −0.2; landing request; grounded → 6. 6: a set flag lets
AI passengers out (players stay); a non-transport closes the hint for 20 s and
its pilot leaves carrying the post, no reboarding for 15 s; a transport (or a
clear flag) flips the flag and waits again. A flyer whose executor reports
stationary (2) is destroyed.

Out of a hangar: the flyer's movement helper (FlyGotoHelper) lifts the craft
off whenever it is moving it, along a path or a straight leg, and the craft is
not flying; stopped, it leaves the craft on its pad. So a pilot waiting for his
crew (WaitSecondary) or in state 3 stays down, and under a roof (Hoth's
hangar) the first thing to move him is state 2's climb out: straight ahead
along the craft's heading, which on Hoth faces the door, at the path's own
heights, until nothing is within 100 m overhead. The flyer input (below) flies
that path as laid.

### Vehicle input conversion

`seatAnalogs`:

- **Hover**: anticipatory ray (1.5 × speed when over 5 m; side push 0.25 D by
  the hit normal; forward halved), the shared translation (arrival radius ½
  ForwardSpeed, square stick envelope with strafe, forward only without),
  facing (the aim with strafe, no target, or arrived; else the movement point),
  the nonlinear hover yaw and pitch (g = 4.5 − clamp(TurnRate, 1, 3), t²
  with sign; pitch g = 5 − clamp(PitchRate, 1, 3)), the MovingTurnOnly
  moving-turn floor, and the reverse-and-turn stuck recovery.
- **Flyer**: `src/flight/ai.js` converts destination and tactical requests into
  local virtual-stick, throttle, roll, boost, trick and target-lock inputs.
  Terrain and obstacles: the original game's altitude stage
  (`raiseOverTerrain`) raises the steering point, never lowers it, onto the
  climb that clears what lies ahead by 25 m at 1.5 s of flight ahead (at least
  1 m): the terrain straight on, at 0.7 of that 45° to either side and at half
  of it 90° to either side (each no lower than the lowest fly height), the
  terrain or water under the craft, and what a ray along the nose from 1.5
  radii below the craft meets past the terrain, counted at its top (the
  original game's bounding-sphere top; the port casts down at the spot) as
  high as the approach makes it, which also slows the craft (the original game
  to 20 m/s, the port to its slow pace, the larger of MinSpeed and a quarter of
  MaxSpeed). With a request of mode 1 the raise eases in nearer than
  2 × speed / TurnRate. The point is then held to the battle's highest fly
  height. The stage is skipped, and the path flown as laid, on a path that
  keeps its own heights (the climb out of a hangar) past its first node (modes
  other than 0, 1 and 3). (Port: the original game also skips it while the
  pilot has a live target; a port bot keeps one for most of his flight, and
  without the stage then bots flew level into what lay ahead.) The port keeps
  one reflex of its own as a last resort:
  with something ahead within 1.5 s (the craft's terrain warning) it pulls the
  nose up and slows, except on a path that keeps its own heights, where
  pulling up flew Hoth's bots into the hangar roof. Landing brakes before
  Space-equivalent activation. Its speeds come from the craft's own range and
  never back it up: braking means its slowest forward flight; a gunship that
  can hover keeps a quarter of its top speed while turning (standing still for
  3 s reads as stuck to the path executor, which destroys a stranded flyer),
  stops only to land, and travels at half its top speed where a released
  stick would hover. On the ground, while its helper moves it (a request of
  mode 1 or 2; a stop writes mode 0), it holds the takeoff key for half of
  each second of the craft's time there (the flight state's clock): a takeoff
  is tried on a fresh press, so a blocked one is tried again every second
  rather than once. The shared quaternion model owns motion, limits, power
  and lock.
- **Walker**: travel held 7.5 s after a yielding contact, forward and body yaw
  only, head pitch from the aim.
- **Stationary controls**: yaw and pitch toward the aim at the control's
  rates (the ordinary branch: error over dt × rate, ±1).

Outputs go to the seat's intent and use the same vehicle physics as the player.
Aircraft use each craft's ODF speeds and handling documented in
[vehicles.md](vehicles.md); a bot's stick pitches the craft at its PitchRate
(a player's mouse at its TurnRate, as the original game has it).

### Vehicle combat

`dispatch` (Combat's hook for sources 2, 5, 6):
walkers pattern 2 against walkers/hovers/buildings in band, weapon choice
only otherwise; hovers the DenseEnvironment policy against vehicles and
buildings (pattern 7 when open terrain, ForwardSpeed ≥ 14 and StrafeSpeed < 5,
else 2 in band), weapon choice only against infantry and aircraft; flyers
against walkers and buildings 15 (an AT-AT, exactly two aboard) or 13 (a
weapon can hit it, no player aboard), weapon choice only otherwise.

Patterns: **7** hover passes (target kept 12 s; legs alternate on arrival or
crossing the entry plane; 10 s without getting nearer falls back to a path;
moving-turn every tick). **13** attack run (A = max(160, 4v), B = max(40, v) set
once per process; P, T − Ad + 90 up, T − Bd, T + Bd + 30 up, capped at the
maximum fly height; fire when forward·delta > 0.5; minimum speed ½v; target
kept 15 s). **14**/**16** dormant (implemented, never dispatched). **15** the
AT-AT tow-cable run (below; target kept 10 s). **Pattern 2's sidestep**
(Findings).

### Pattern 15: the tow-cable run

How the 2004 game flies a snowspeeder round an AT-AT
(`src/ai/classic/vehicles/orbit.js`, flown by `steering.js` and
`src/flight/ai.js`):

- **The request**: the tracker's target, fresh or not, a signed radius ±25 and
  a factor, and an offset in the target's frame. Before the craft's cable
  holds, the offset is (±12.5, 0, 25) and the factor 2.5; once it holds,
  (0, 0, 8.75) and 1. The two signs are drawn at entry.
- **The point**: the centre C is the target's position 7 m up plus the offset
  turned into its frame. The ellipse is |radius| across and |radius| x factor
  along the target's forward A. The craft's flat position is squashed along A
  by 1 / factor about C, so the ellipse is a circle of |radius|. From outside
  it the tangent point is taken: the bearing from C, atan2(dz, dx), less
  acos(r / L) for a positive radius, plus it for a negative one, out along
  (cos, sin). From inside, the point straight out on the circle. It is
  stretched back by the factor, then moved |radius| along the ellipse there
  (across its normal, the normal being the radial direction squashed by 1 /
  factor): less (−n.z, 0, n.x) x |radius| for a positive radius, plus it for a
  negative one. Its height is C's.
- **The movement request** (mode 3): with t = (the craft's flat distance from
  C) / (factor x the signed radius) − 1, the request's height share is
  clamp(t / 3, 0, 1) (the altitude stage takes that share of its climb for
  mode 3; mode 1 asks it for its distance blend with −1) and its movement
  scale clamp((t − 1) / 5, 0, 1), which the flyer input turns into
  2 max(scale, 5 / MaxSpeed) − 1. Near the walker both are 0: the snowspeeder
  runs at its slowest (about 23 m/s) 7 m over the walker's feet, among its
  legs. With a negative radius t is below 0 everywhere, so that run is slow
  and unraised all the way in.
- **The tow pass**: while (the craft's flat forward · A > 0) XOR (offset x >
  0) differs from (radius > 0), and (A · (C − craft) > 0) differs from
  (radius > 0), every control of the hull that has a `WeaponTowCable` selects
  it. With the offset +x and the radius +, that is the near pass along the
  walker's flank, the walker's way, once past the centre: the walker is behind
  the craft, in the rear gun's arc. (Port: only the seats bots hold; a person
  keeps the weapon he chose. Of the four drawn sign pairs, the one with the
  offset −x and the radius + makes its tow pass running at the walker, which
  the rear gun cannot reach; its gunner still takes the harpoon itself against
  an AT-AT, which it scores 50 to the cannon's 12.)
- Measured on Hoth (`node tools/tow-cable-probe.mjs`, the Rebels, Medium,
  5 minutes, seeds 1 to 3): 8 harpoons, 7 held, 3 AT-ATs fell; without a
  steered orbit and the reach check below the same run gave 30 harpoons, one
  held and no walker down. With a second bot seated behind every bot pilot
  (`--crew`, 4 minutes, seeds 1, 2 and 4) 5 fell. With the player flying past
  an AT-AT 15 m off its legs at 40 m/s
  (`node tools/tow-gunner-scenario.mjs --pass --radius 15 --speed 40`) his bot
  gunner harpooned it on three passes in 45 s.

Port changes for the run, each beside the original game's rule it changes:

- **The tow focus** (`perception.js` `TOWED_PRIORITY`): a pilot whose
  craft's cable holds a walker ranks that walker above anything (cost 0).
  Ranked as the original game ranks (the nearest first, every walker, turret
  and craft alike), a pilot towing on Hoth was drawn off by a nearer AT-ST,
  turret or the other AT-AT within a dozen seconds, before three circuits,
  and the cable stretched past its length behind him.
- **The harpoon's reach** (`weapons-ai.js` `harpoonReach`): a bot fires the
  tow gun only with its target within the harpoon's cast (LockOnRange 70,
  at least 20). The port casts the harpoon (src/vehicles.js
  `Vehicles.launchTow`); the original game's flies as a round, and the AI's
  range for the gun is its MaxRange, 300 m, so bots fired harpoons that
  could not reach, each costing the gun its 3 s ShotDelay.
- **The rear gunner's rest** (`Vehicles.board`): a bot takes a seat at its
  rest heading (`Vehicle.restYaw`, the middle of an arc that leaves the
  hull's heading out), as a person does. The hull's heading lies outside the
  snowspeeder rear gun's YawLimits 80 280, and a bot that kept it as its home
  look had its gun, with no target, sit against a limit and swing from one to
  the other as the craft turned.
- The original game's rules the port also follows: the tow gun takes no
  trigger while its cable holds (`Vehicle.towHeld`), and the cable lets go
  only when it is stretched too far or one end dies (`Vehicles.updateTethers`;
  the cable's run is tested only against the walker it holds).
  docs/vehicles.md "The snowspeeder's harpoon and tow cable".

### Unattended decay

`updateDecay(dt)`: a hull outside the boundary, or one on a pad linked to a
post that has team 0, with nobody aboard, gathers one idle sample a second; a
player who could board it within its radius plus his plus 10 m pauses the
count; past 20 it loses MaxHealth / DecayTime (10) a sample; anyone aboard
clears it.

## Integration

`src/game.js` registers hulls and turret targets, creates `TurretSeats`,
exposes `world.vehicleSystem`/`world.turretSeats`, and routes mounted shots,
damage, boarding and exits into the simulation journal. Capture and blast
soldier loops skip non-soldier registry entries. Repair services see actual
hulls and buildings. `humanTarget(vehicle)` exposes the player's reticle
target to his AI gunners.

Every simulation step runs AI decisions/input, on-foot unit movement, then
vehicle and turret input batches. Seated bodies skip soldier physics and
leave their intent edges for the seat. The Classic entrypoint applies mounted
aim filtering and vehicle decay each step. Rider presentation stays in the
render frame. `Unit.origin/forward` delegate to its seat, and spawning clears
seat/mounted/targetable state for the new life.

`ordnance.fire` accepts a seventh emitter argument: a beam follows the barrel
that fired it (the seat's own emitter, on the hull, for a weapon with no
barrel node) while its owner remains the firing soldier. Every body-impact
callback carries that owner. Human and AI mounted shots therefore share kill
credit; standalone unitless vehicle fixtures keep their hull owner fallback.

`createAI` invokes `adaptClassicAI` after module binding. The adapter supplies
the controller hooks below on the instance.

### Controller hooks (C1 to C5)

`adaptClassicAI` does these on the instance:

- **C1** `ClassicAI.sync()` and `spawned()`: use
  `this.vehicles?.currentControllable?.(character) ?? character.unit` as the
  Character's unit, so boarding rebinds its controller to the seat control (a
  fresh bind, as the original game builds a controller per control) and
  leaving rebinds it to the body.
- **C2** `UnitController.bind(unit)`: `this.policy =
  this.ai.vehicles?.controllerPolicy?.(unit) ?? null;
  this.policy?.bind?.(this, unit);`
- **C3** `driveCommands`/`goalToState`: `const state = this.policy ?
  this.policy.goalState(goal.kind) : goalState(goal.kind)`.
- **C4** `onEnter`: `this.policy?.agentFor?.(this.state, this) ??
  this.ai.agentFor(this.state, this)`.
- **C5** the scheduler's input step: for a seat control
  (`unit.lease?.controllable === unit`) call `ai.vehicles.analogs(unit,
  control.request, aimPoint, step, out, { controller, stepped: true })` after
  `control.update(step)` in place of Nav's `navigationInput`, and in place of
  `ai.analogs` in the plain path (which also runs the executor step).
- Add to `VEHICLE_SERVICES`: `controllerPolicy`, `analogs`.

### Combat

Reads `world.vehicles.dispatch/enterPattern/tickPattern/exitPattern/sidestep/
sharedTarget/unreachableFromMount` and `ai.vehicles.mountReach`; the binding
installs the service at `world.vehicles`. `sharedTarget` needs the player's
target from `world.vehicleSystem.humanTarget(vehicle)` (implemented: what is
under his view).

### Population

`ai.controls()` (mission controls): `vehicleNotifyRadius`, `stayInTurrets`,
`stationaryWait`, `dense`, `flyHeight.{min,max}`.

## Findings

- **Soldier boarding eligibility**: the candidate alive and an unexplained
  value of its own not positive (not modeled); a candidate whose team is above
  0 takes an AI soldier of that team only, a player when the relationship (> 0)
  allows; then the first seat in the candidate's control list with PilotType 2
  that has no pilot, or (for a player) an AI pilot.
- **Entering a seat**: the same choice; a displaced AI pilot first tries the
  next free seat and otherwise exits (no promotion, with placement); inputs
  reset; a seat class flag decides whether the soldier's own controller is
  destroyed; the soldier becomes the seat's pilot (SetPilot). No reach test:
  proximity is the agent's.
- **UnitBoardAgent**: as described above; the speed gate is the candidate's
  velocity squared below 4.
- **UnitUnBoardAgent**: as described above; its tick only updates the target.
- **UnitFlyAgent policies**: no danger, alerts, sounds or boarding; a scanner
  target only outside the climb-out and landing states (2 and 5) and never for
  a transport; the crew wait yes; commands and recovery as the base; the
  interrupted goal is saved.
- **PilotType** maps none 0, self 1, vehicle 2, remote 3 (value hashes
  `ada7afdb`, `645ba277`, `71cd882f`, `c78d7953`); a Controllable class
  defaults it to 2 and its controls copy it. The promotion test is
  PilotType == 2. NoEnterVehicles (`939b9985`) sets a class flag to
  (value == 0): the notifier's "may enter vehicles" flag.
- **Exit request**: calls the pilot's exit with promote = (the controller's
  current goal kind is not 5) and placement on, then assigns the saved post on
  success.
- **Capacity**: a hull's seat count is its control list's seat count; the
  occupants subtracted from it are the player-controlled ones only, so bots
  aboard are not subtracted.
- **Notifier**: confirms the bucket counter (init N, N → 0), the full-seat
  skip, speed² ≤ 9, an unexplained vehicle flag (not modeled), the two barrier
  tests for non-buildings (radius 2 with mask 1, then the traversal radius),
  the query (the notify radius, cap 30, mask 6), the controller/class gates,
  the owner team override needing an allied relationship unless a player
  pilots it, the low-ticket skip, and delivery through the controller. Armed
  buildings are in the scanned list (the building test skips only the barrier
  checks).
- **SetPilot**: copies the pilot's team to the vehicle; clearing the last
  pilot sets the vehicle's team to 0 under a flag whose meaning is unknown.
- **Pattern 2's sidestep**: a hover or walker with ForwardSpeed < 13 and
  StrafeSpeed > 0.0001, when the per-second chance helper (1, 13) returns 1,
  moves (direct goto) to P ± right × uniform(r, 2r), r its bound radius, the
  side from an integer 0..1 draw.
- **The per-second chance helper** (lo, hi): lo when lo == hi, else a random
  integer from lo to hi × G (G the AI cycles per second).
- **Flyer target reservations**: priority 3 for a Flyer owner rejects a target
  with a live type-2 reservation by its team and otherwise reserves it (type
  2).
- **The hover's own aim**: `PitchRate`, `PitchLimits` and `YawLimits` (degrees,
  read as radians; class defaults 0.5 and ±0.5 rad). Each update the aim pitch
  grows by PitchRate × the pitch input × dt (the yaw likewise at the turn
  rate), is turned back by the hull's own turn that step (the old right and up
  axes dotted with the new forward, so the aim holds still in the world), and
  is clamped to the limits as an angle from the hull. The aim angles are
  zeroed with the hull and nothing on a pilot's entry resets them. A bot
  boarding a hover takes its aim level (`Vehicles.board`). The player view's
  little-down rest (−0.12 rad, the port's choice) was held at −5 degrees by
  the speeder bike's `PitchLimits -5 2`, and the nonlinear pitch analog
  (pitch = t², so 0.01 for a 0.025 rad error) lifts it at a few hundredths of
  a radian a second at best, so the bikes' bolts went into the ground in front
  of the bike they fought: over 3 seeds of 3 minutes on Endor, 18 % of the
  bike bots' shots met the world before their target with that rest, 5.5 %
  with the level aim. Not ported: the port keeps the hull seat's pitch in the
  world and clamps it there, not from the hull; with the port's hull bobbing
  more than the ground under it, clamping from the hull made the bikes' aim
  worse (26 % in the same test), so that waits on the hover's pitch physics.
- **Why bots leave walkers** (the port matches each):
  - Patrol's state 1, mounted: the control's speed against 12.5; under it, a
    query of 30 m, capacity 10; a candidate with a Character redirects the
    post (exclude flag set, with the current post) and keeps the seat.
    Otherwise, and for anything faster, the exit request, next boarding at
    now + 5, finish 100. Every walker is under 12.5, so in the original game
    too a walker pilot gets out at a post with no enemy soldier within 30 m;
    at a held one he stays and is sent on.
  - Low reinforcements: teams 1 and 2 with reinforcements at or under 25 count
    as low, and the seat is kept only when that is false or the control's
    PilotType is not 2.
  - The AT-AT never patrols (Trooper state 2 stops it), so it never takes
    the patrol exit.
- **Walkers stand to shoot** (pattern 2's entry): it stops the movement helper
  and draws the crouch from the controller's roll. So a walker halts for a
  hull or building in its weapon's band (the AT-TE's heavy cannon 20 to 250 m)
  and walks on while it only fires at soldiers (pattern 0). AT-TEs on
  Geonosis firing at the Techno Union ships from about 250 m is that band.

## Provisional

- `src/vehicles.js`: an empty hull keeps its spawn team (the original game can
  set it to 0 when its last pilot leaves, under a flag whose meaning is
  unknown); the registry team is the first occupant's while occupied.
- `src/vehicles.js`: `lastAttacker` keeps the last attributed damage
  (unattributed damage leaves it), so an environmental kill after a hit is
  credited to that attacker.
- `src/vehicles.js`: bot seats use the AI spread floor; vehicle seat
  turn/pitch rates are the seat's (class) TurnRate/PitchRate (the player's look
  is mouse-direct).
- `src/vehicles.js`: an empty flyer sleeps only when grounded (it used to
  freeze in mid-air if its speed passed through zero after the player got out
  while climbing).
- `src/seats.js`: the exit frame forward is a turret seat's own heading
  (TurretNodeName) and otherwise the hull's; the bounding radius is half the
  collision box diagonal; the turret target team when unmanned is the
  building's (0).
- `src/ai/classic/vehicles/steering.js`: the walker's body yaw and head
  pitch map onto the port's single seat aim; touching bounding spheres stand
  in for the walker's collision callback; renewed requests (moving-turn,
  minimum speed, loop) lapse 0.5 s after their last renewal; aircraft requests
  feed the shared flight controller; pattern 15's orbit (see "Pattern 15: the
  tow-cable run") steers until the pattern clears it, as the original game's
  does, without the renewal lapse.
- `src/ai/classic/vehicles/index.js`: the notifier's barrier tests use Nav's
  `pointClear(p, 2, 1)` and `pointClear(p, navRadius, 1)`; turrets come before
  vehicles in the scan order; decay's special 10 s threshold (a CP flag) is not
  modelled; `sharedTarget` uses what is under the player's view.
- `src/ai/classic/vehicles/agents.js`: the flyer's post ownership test is
  "the post's team is friendly"; a closed hint is reserved until the close
  time.
- `src/ai/classic/vehicles/patterns.js`: the AT-AT test compares the label
  "AT-AT" or the stock class name; pattern 15's parity draws use generator A.

Port changes against vehicles and boarders that stopped moving (found with
`tools/ai-stuck-probe.mjs`, docs/ai.md "Stuck bots"; tests in
`tests/ai-stuck.test.js`):

- `steering.js`: the facing choice's "already arrived" is the slowdown's own
  condition, inside the near radius with no path left. Read as "inside the
  radius" alone, a walker (which walks only where it faces) stood for good
  within 4 m of a waypoint on its way that it had to come within 1 m of,
  when that waypoint lay beside or behind it. Hovers without strafe use the
  same rule.
- `steering.js` (`translate`): a unit on its steering point itself is near
  (arrived). A stop is written where the unit stands, and the zero-length
  early return left near false, so a hover or walker told to stand and shoot
  (pattern 2) faced the point under it instead of its aim: a speeder bike's
  fixed cannon never came to bear, and it sat by its target unfired for as
  long as the target stayed.
- `steering.js`: a walker pressing on and going nowhere (its executor
  stationary while it asks to walk, not held) takes the hover's
  reverse-and-turn recovery. Classic gives walkers none; a tauntaun against
  a bank it could not climb stood there until killed. So does one turning
  before it walks whose hull has not turned 0.01 rad in 2 s: the hull
  follows the pilot's aim (`src/vehicles.js` steer), the aim already faces
  the path so the turn analog is 0, and a hull against a wall cannot turn
  (the push-out restores its yaw when a probe sinks more than 0.15 m), so a
  tauntaun wedged facing away from its path stood for over 3 minutes.
  Only status earned while pressing on counts: the walker must have asked to
  walk for the executor's 3 s stationary window first. That count also runs
  while a walker turns on the spot before it walks, and a slow turner (an
  AT-TE or spider walker at 0.6 rad/s: over 5 s for a half turn) set off
  with status 2 already up. It backed up turning, turned back to its path
  for longer than the 4 s between recoveries, and backed up again, never
  walking where it faced: on Geonosis spider walkers drifted 300 m the wrong
  way and off the map, and the AT-TEs, caught in the same loop each time
  they set off again after a fight, never came within 270 m of the Techno
  Union ships.
  Recoveries in a row that end where the last one did (within 10 m) mean
  the place is out of that hull's reach. On Kashyyyk Docks the post cp4
  stands on the raised dock, and one planning barrier (`barrier932`,
  123 x 33 m, every vehicle size but not soldiers) covers that dock, so a
  vehicle's approach point is pushed out of it (`BarrierSet.correct`,
  ported as the original game has it): the nearer edges are covered by other
  barriers, and the point lands 109 m away at the foot of the quay wall,
  under its roof. A spider walker sent there backed out and walked back in
  for six minutes. The nav data and the collision agree (the dock is 24 m
  above the quay floor there), so nothing is wrong with the extract. After two such recoveries
  its movement helper plans the trip again (new waypoints on the hubs);
  after four the pilot asks for another post (`requestObjective` with the
  exclude flag), through `env.replan` from `VehicleAI.analogs`. In that
  battle its recovery time went from 174 to 35 s of 386 at the controls.
- `index.js` (`walkerContacts`): the touching-spheres stand-in counts as a
  collision only while this hull moves into the other. Spheres reach well
  past the hulls, and a walker held beside a larger hull at rest (a parked
  one) renewed its 7.5 s hold every step and never walked again.
  The stand-in is now the two hulls' collision boxes seen from above, at
  their headings, within 0.5 m of each other (provisional). Bounding
  spheres take in a hull's height: an AT-AT's (8 x 20 x 21 m box) reached
  15 m to every side of a body 4 m wide, and on Hoth an AT-ST escorting
  one 20 m off was held for 136 of 786 seconds at the controls (two
  8-minute battles); with footprints, 14 of 590.
- `index.js` (`patrolMounted`): the slow-walker branch asks for a post other
  than its own (the exclude flag with the current post, as the original game
  does); asking with no exclusion drew the same post again.
- **Posts out of a hull's reach** (`support.js`; `UnitTrooperAgent.approach`
  in `../agents.js`, `index.js` `supportPoint` and `patrolMounted`; tests
  in `tests/ai-vehicle-support.test.js` and `tests/ai-vehicles.test.js`).
  On Kashyyyk: Docks every post the defenders start with (cp3, cp4, cp5)
  stands on a pier under planning barriers of every vehicle size
  (`barrier921` to `barrier966`, flags 62). For an AT-ST, an AAT or a
  spider walker the barrier correction leaves the cp3 and cp5 points inside
  a barrier and pushes cp4's 107 m away, under the quay wall. The walk
  ended short, the agent took the end of its path for arriving, and
  Patrol's mounted branch, with no enemy soldier within 30 m, put the pilot
  out. The next bot to board walked to the same spot and got out again:
  in two 10-minute battles 27 pilots got out that way, and the Empire's
  AT-STs stood empty in the shallows by Dock Two. Now a pilot of a walker or hover (`unit.kind` 2 or 6)
  whose post is out of reach (the correction fails, or moves the point
  more than the capture region's larger half-size plus 10 m) walks
  to a support hub instead: a hub of its traversal mask within 100 m of
  the post, clear of its barriers at its radius, from which a point 1.5 m
  over the post's center can be seen at the hull's eye height, and to
  which its routing table has a route from where it stands. It picks at
  random among those within 15 m of the nearest, so a team's hulls spread
  out. The approach offset reaches that point,
  so arriving is the agent's usual test. There Patrol's mounted branch
  keeps the seat. While the post is not its team's, the pilot stands
  (patrol state 6, 20 to 30 s) and fights whatever comes into sight, three
  times. After that, or once its team holds the post, it asks for another
  post (exclude flag), still aboard. On Kashyyyk: Docks the support hubs
  are 66 m (cp4), 69 m (cp3) and 80 m (cp5) from the posts, on the dock
  level. In the same two battles (`tools/walker-probe.mjs kas2 --side imp
  --minutes 10 --seeds 1,2`) no pilot got out by request, and the AT-STs'
  crews did 25,419 damage against 12,285. Posts in reach and every other
  map's approach are unchanged.
- `agents.js` (`UnitBoardAgent`): once the walk is over with the seat still
  out of reach (a turret on the deck above the point its route ends under),
  the bot gives the vehicle up and takes no invitation for 15 s. The
  reservation renews every tick, and it stood there for good.
- `agents.js` (`UnitFlyAgent`) and `index.js` (`abandonPad`): a stranded
  flyer (executor status 2) is destroyed only once off the ground. One still
  landed on its pad, its takeoff failing however often its pilot tries,
  is left by its crew instead: every bot aboard gets out through the
  boarding rules everyone follows (Vehicles `unboard`, and `requestExit`
  for the pilot, whose agent is running; src/boarding.js `mayLeave`),
  nobody moves up to the controls, the agent ends and they go on foot; none
  boards again for 15 s, and no bot is invited into that hull for 60 s.
  People aboard stay. The original game destroys a stranded flyer, which suits
  one stuck in the air; on Bespin a gunship whose takeoff was blocked blew
  up on its pad with its crew about once a minute, coming back on the same
  pad each time.

## Unknown (hooks left, nothing invented)

- The notifier's unexplained vehicle flag.
- The unexplained condition on the candidate in boarding eligibility.
- The VehicleSpawn flyer lifecycle's reference branch and the command-post
  class's bit 3 region test.
- A vehicle class flag muting interrupt priorities 1 to 4 (the Controller's
  `alertsBlocked` hook stays false).
- How the player's weapon target is chosen for the shared mounted target; the
  reticle stands in.

## Tests

`tools/tow-cable-probe.mjs` (the snowspeeders' crews, runs and harpoons
in a headless Hoth battle) and `tools/tow-gunner-scenario.mjs` (a bot rear
gunner while the player circles or passes an AT-AT) measure the tow-cable
run.

`timeout 300 node --max-old-space-size=1024 --test tests/ai-vehicles.test.js`
(set `NODE_OPTIONS=--max-old-space-size=1024` so the per-file child inherits
the cap). Identity assertions in that file never inspect their operands: a
failing `assert.equal` on a unit, lease or vehicle would print its whole graph.
`tests/vehicles.test.js`, `tests/turret.test.js`,
`tests/vehicle-assets.test.js` and `tests/vehicles-browser.mjs` (with
`PLAYWRIGHT_MODULE` set) cover the player's paths.

Regressions cover seeded turret projectile spread, lease-scoped beams, human
takeover of AI weapon settings, and destruction/rebuild target generations.

## Contract changes

- docs/ai.md `unit.seat`: the seat lease (`.vehicle`/`.turret`, `.index`,
  `.role`, `.origin(out)`, `.forward(out)`, `.controllable`).
- Unit field `targetable` (false while seated).
- ActorIntent vehicle fields: `brake` (held), `up`, `down` (held, 0/1), and
  `exit` (edge, same as `use` in a seat).
- World services: `world.vehicleSystem`, `world.turretSeats`; `world.vehicles`
  is the vehicle AI's service (Combat's hooks).
