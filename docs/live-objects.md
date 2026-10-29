# Live objects: service droids and the repair tool

How the power-up droids (`src/props.js` PowerupStation) and the fusion
cutter (`src/combat.js`, `src/weapons.js`) show their work in a battle that a
world host (the local server worker, or a server) runs and the page shows.

## What travels, what is derived

The page gets each building's plain state in the entity protocol's world
presentation records (`src/runtime/entity-world-state.js`) and the effect and
sound calls the tick made as presentation events
(`src/runtime/entity-presentation-events.js`). It never ticks the buildings
itself. So:

- A droid's idle cycle and serve pulse are state only (`update` and `step`:
  `mode`, `t`, `direction`, `rate`, `pulse`, `yaw`, `servedPlayer`,
  `servedHealth`, `servedAmmo`). Its rig is posed from that state by
  `pose()`, which accumulates nothing, so the page and the world agree
  whoever calls it and however often. `LiveObjects.present(dt, viewer)` poses
  the living droids and updates the build holograms every frame
  (`game.js`, in the world presentation after the posts); the tick's
  `LiveObjects.update` does not touch holograms.
- A pulse that gave someone on foot something calls the station's `onServe`
  hook; `game.js` plays the soldier's `HealthGainEffect` (default
  `Powerup_blue`) and `AmmoGainEffect` (`Powerup_red`) at his feet, the
  same effects a power-up plays (`src/units.js`). The PowerupSound
  (`emitSound("serve")`) travels as a `serve` presentation event.
- The HUD's supply indicator is derived on the page: `combat.updateHud` asks
  `live.supplying(soldier.position)` for a station whose last pulse served
  the player (`supplied.player === true` in `game.js`) while he is still
  within its radius, and `hud.setSupply(health, ammo)` pulses the health bar
  and the rounds green and keeps them shown.
- The repair lock is also a plain field, carried in the unit's entity state:
  `combat.repairLock` is `l{i}` for a building or `v{j}` for a vehicle
  (`repairLockKey`), and `combat.lockedTarget()` resolves it on a page that
  never ran `lockOn`. The second health bar (the target's health, a pad's
  build progress) shows on such a page because of this.
- The cutter's sparks (`MuzzleFlashEffect`, `repairkit`) are the page's: the
  world host has no posed hand, so `presentRepairTick` plays them at the
  tool's fire point as drawn (on the tool in the hand or, in first person, on
  the arms' tool) when the tick's shot reaches the page. The FireLoopSound
  rides on the shot's `fire` sound event.

## Turrets: damage effects, pieces and sounds

A placed turret (`src/props.js` Turret) shows and sounds as a hull's turret
does, on the page in every mode, from what every host gives it: its state
(built, destroyed, unbuilt), its health, its lease and its drum's angles.

- The original reads an armed building's class through the building reader
  (`ExplosionName` or `Explosion`, `DestroyedGeometryName`, the build pad),
  which hands every other key to the reader hulls use too: `CHUNKSECTION`
  and `Chunk*`, and the `Damage*` records. Its destruction swaps in the
  wreck (or the pad), sets off its blast and throws its chunks through the
  same routine as a walker's end. Its seat is the same turret component as
  a hull's turret seat (`TurretActivateSound`, `TurretDeactivateSound`, the
  whirs).
- `tools/extract.py` `write_props` gives each instance its class's
  `fx.damage` and `fx.chunks` (`tools/zero_vehicle.py` `vehicle_fx`), writes
  the effects they play in `props.json`'s `effects` (a fallback library the
  page merges under the shared ones) and the pieces' models in the model's
  space in `props_extras.bin` (`props.json`'s `extras`).
- `src/vehicle-effects.js` `updateBuildings` runs each turret through the
  hulls' rules: each damage record burns at its node of the built model
  (the model's root where it lacks the node: the AAA, Hoth and gun turrets
  have no `hp_smoke_1`, so theirs rise from the base) while built and its
  health is in the record's band; destroyed, its pieces fly (`Debris`),
  passing through the wreck they start inside; built again, a new life.
- `src/audio/vehicles.js` `updateBuildings` sounds whoever mans it, the player,
  a bot, or another player online: `TurretActivateSound` as the lease arrives,
  the yaw and pitch whirs while the drum turns (at its `MaxTurnSpeed`),
  `TurretDeactivateSound` as the lease goes. Destroyed while manned, it falls
  silent without a sound. The player's own turret has no separate path in
  `src/audio/scene.js`.

What the retail classes give:

| Turret | Maps | Damage effects | Pieces | Blast | Turret sounds |
| --- | --- | --- | --- | --- | --- |
| `bes_bldg_AAA_turret` | bes1 | `vehiclespark` 50-30 %, `smokeplume` 30-0 % (`hp_smoke_1` missing: the root) | three (`antiaircraft_chunk2` to `4`, trails `mediumsmoketrail`), wreck `antiaircraft_chunk1` | none: it names `bes_bldg_AAA_turret_exp` only as `ExplosionDestruct` and `ExplosionCritical`, which the building reader does not read | activate, deactivate, whirs |
| `com_weap_gunturret` | bes2, end1, kam1, kas2, nab1, nab2, rhn1, rhn2, tat1, tat2, yav1, yav2 | the same two (the root) | none | `com_bldg_gunturret_exp` | activate, whirs (no deactivate) |
| `geo_bldg_geoturret` | geo1 | the same two (the root) | none | none | activate, whirs |
| `hoth_bldg_dish_turret` | hot1 | the same two (`hp_smoke_1`) | none | `hoth_bldg_dish_turret_exp` | activate, deactivate, whirs |
| `hoth_bldg_hoth_turret` | hot1 | the same two (the root) | none | `hoth_bldg_hoth_turret_exp` | activate, deactivate, whirs |
| `tat_bldg_barge_turret` | tat2 | the same two (the root) | none | `com_weap_bldg_gunturret_exp` | none in its class |
| `yav_bldg_tower_turret` | yav1 | the same two (`hp_damage_1`) | none: its one `CHUNKSECTION`'s keys are written `\\ChunkGeometryName` and so on, names no reader knows | `yav_bldg_tower_turret_exp` | activate, deactivate, whirs |

## Turret gunners

A placed turret's gunner is a rider of the turret, by the rules of a vehicle
seat's (docs/vehicles.md "Riders in the open", docs/formats.md "Riders"): the
armed building's update poses its pilot through the vehicles' pilot section, so
he shows only where the class names a pilot animation, and rounds, melee swings
and blasts meet him only where he shows. The rules are the same for every
gunner: the player at his `Gunner` (`src/gunner.js`), bots and network humans
at `TurretSeats` (`src/seats.js`).

- Shown (`Turret.rider`): at `PilotPosition`, in `human_` and his
  `Pilot9Pose`, by the drum's pitch and turn speed. `Turret.riderSample` is
  the one pose he is drawn and struck in. Hidden (`Turret.hidesGunner`): the
  AAA and Hoth turrets name no pilot animation, so whoever mans them is out
  of sight and out of reach, as in the original (the pilot section hands the
  soldier no pose without one).
- Placed and turned (`Turret.seatPoint`, `Turret.seatTurn`): the seat node's
  place and its whole turn in the world, as the pilot section hands the soldier
  the node's transform and he takes it for his own matrix: the drum's turn, and
  the lean of a turret placed on a slope (one Hoth dish turret stands 9 degrees
  off level). The clip's root offset isn't his (docs/formats.md "Riders"). The
  drum's rest pose keeps no heading of its own (`Turret.drumBind`): the barge
  turret's `aimer_01` is bound half round, which would leave its gun pointing
  back over the deck against the view and its gunner hanging outside the hull,
  facing away from the gun; instead the gun points out over the rail and he
  stands on the deck behind it.
- Drawn the same on every page: bots and other people by `TurretSeats.pose`
  (the world, and a page from the turret's record, whose turn speed is the
  yaw's change between records, `Turret.aimFrom`); the player by
  `Gunner.update` in the world and by `Gunner.poseBody` on his own page, at the
  aim the page previews, so the page poses its own player at the gun in the
  gunner's pose the world strikes him in.
- Struck (`Turret.exposedSeat`, `Turret.riderShapes`, `SeatLease.exposed`):
  his posed body at the seat, in the seat node's frame
  (`src/rider-pose.js`): head, torso, thighs, shins and both arms to the
  middle of the hand. The turret's own collision is the nearest hit where it
  lies in front of him: the gun turret's tub covers him to the waist (its
  collision is a plain drum about 1 m tall, without the shield plates or the
  gun); the Yavin tower's walls cover him to the chest. The turret's own
  guns never strike him.
- Blasts (`SeatLease.blastable`, game.js `onExplode`, `Combat.explode`): the
  original's soldier area damage hurts a seated soldier only when the seat is
  an armed building's, and its push skips a soldier in a seat. A shown gunner
  takes a blast's damage at the gun without being moved. One the blast kills
  has left the gun by the time it pushes (his death in the seat runs the seat
  exit), and his body is thrown as any other.
- Killed at the gun (`src/boarding.js` `leaveKilled`: `TurretSeats.vacate`
  with reason `killed` for a bot or a network human, `Gunner.exit` with
  `killed` for the player), or with the turret destroyed under him, he
  leaves it at once and no spot beside it is sought: his body falls from
  where he stood (under his pelvis, his soles where his feet were drawn),
  his death clip under way, and the turret waits for the next gunner.

| Turret | Gunner |
| --- | --- |
| `com_weap_gunturret` | `minigun_9pose` at `hp_active`, in the tub up to his waist |
| `geo_bldg_geoturret`, `yav_bldg_tower_turret` | `minigun_9pose` at `hp_active` |
| `tat_bldg_barge_turret` | `minigun_9pose` at `hp_active_01`, on the deck behind the gun at the rail |
| `hoth_bldg_dish_turret` | `dishcannon_9pose` at `hp_active`, on the step behind the dish, hands up on the housing's grips |
| `bes_bldg_AAA_turret`, `hoth_bldg_hoth_turret` | hidden |

In the headless world a clone trooper (300 health) at a minigun turret dies
to six 50-point rifle bolts in the chest or two in the head, from any side
where the tub leaves him in sight. A thermal detonator going off 2 m from
him kills him and throws his body; a rocket's blast at 2 m takes 92 health
and leaves him at the gun. `tests/turret-gunners.test.js` covers the rules,
and for every retail class that shows its gunner, that his drawn head,
hands, elbows, pelvis, knees and feet lie inside his hit shapes at any
aim. `tools/turret-body-probe.mjs` draws a player's and a bot's gunner from
beside, before and behind the gun with his hit shapes over him, and
measures how much of the drawn body they hold (about 88% of the trooper's
vertices within 2 cm; what they leave out is the toes, the shoulder pads
and the chest's edges).

The dish gunner's clip holds one pose per turn column, the same in every
row (he doesn't follow the dish's pitch), and its ninth frame is a rest
pose: tipped down (10 degrees at most) and turning left, the port blends it
in, about a fifth of the way at the most, as the original would if it
reads that frame (not checked).

## Turret views and aim

An armed building's turret is the seat object a vehicle's `TURRET` section
makes: the building's class parser hands every key after `BuildingSection` to
the seat's parser, which reads the camera keys through the camera section
parser and the fields of view through the control section's. Its camera is the
seat's and its aim the seat's update, so a placed turret looks and aims by
docs/formats.md "Vehicle seats" (`src/props.js` Turret, `src/gunner.js` Gunner
and TurretCamera, `src/seats.js` TurretSeats).

- **The aim frame** hangs at the drum's pivot (`TurretNodeName`, its place in
  the bind pose), looking along the aim, its up the drum's. The aim's yaw is
  measured from the building's heading and held to `YawLimits` when the arc
  lies inside ±180 degrees (the barge turret's ±70; with none, the default
  ±10 rad turns freely). `PitchLimits` is down positive, as for every turret
  seat: the Yavin tower looks 10 degrees up and 35 down, the dish turret 10
  up and 25 down, the AAA 30 up and 20 down.
- **The drum** is posed as that frame (yaw by `YawTurnFactor`, pitch by
  `PitchTurnFactor`, 0 on every placed turret): its bind turn is not kept,
  so the barge turret, whose drum's bind pose faces backwards, turns its gun
  along the aim like any other.
- **The camera section.** `EyePointOffset` is the eye in the aim frame and
  sets `TrackCenter` too; a `TrackCenter` after it overrides it. Unset keys
  are 0 (`TiltValue` included), and both fields of view are 48 degrees. A
  vector key is read as `sscanf("%f %f %f")` reads it: the dish turret's
  malformed `0.0 1.5.0 -2.0` is (0, 1.5, 0).
- **Third person:** the camera looks along the aim, pitched down by
  `TiltValue`, from `TrackCenter` in the aim frame plus `TrackOffset`
  (right, up, back) in its own. Its turn follows the mouse at once; each
  axis eases to where it should be at 30/s. Five rays from 1.5 m up in the
  aim frame, the turret left out, pull it in at walls; it eases back out at
  6/s.
- **First person:** at the eye, straight down the aim. An eye inside the
  turret's box (the Hoth, dish and Geonosian turrets) takes the drum and gun
  out of the view, as a vehicle's cockpit does; one above or behind it (the
  Yavin tower's, the AAA's) keeps them in view.
- **The aim point** is the first wall, prop, hull or soldier within 512 m
  along the aim from the person's own view: his eye in first person, else
  where his chase camera rests, brought forward to the eye's plane. The
  page's `Gunner` and the server's `TurretSeats` (for a network human, whose
  view arrives with his input) aim from the same point
  (`seats.js` `turretAimPoint`), so the gun turns to what lies under his
  crosshair on either. The page's reticle points from the drawn camera at
  that point (`Gunner.reticleDirection`), so it stays on it while a wall
  pulls the camera in off its rest or the camera eases after a turn. Bots aim from the gun's fire point along their own
  aim.
- **The gun** (`AimerNodeName`) turns toward the aim point from its rest pose
  within `AimerYawLimits` and `AimerPitchLimits`, which the aimer reads up
  positive (±30 and ±45 degrees unless set). The port widens its pitch limits
  to every pitch the view can take: the Yavin tower's, dish turret's and AAA's
  classes write the same numbers for the gun's limits and the view's, which the
  original reads with opposite signs, so there their guns stop short of the
  crosshair (the Yavin tower's at 10 degrees down, its view at 35; the dish
  turret's at 10 down, its view at 25). This departs from the original in every
  mode.
- **Rounds** leave the fire point straight at the aim point once the gun has
  turned as far toward it as it can (`Turret.fireDirection`): when it bears on
  the point within a degree, as a vehicle's barrels do, or when it stands at
  its limits short of it (the AAA's and dish turret's guns never yaw; the dish
  turret's gun stops 25 degrees up under its high chase camera). So they land
  under the crosshair from either of the AAA's barrels, at any pitch the view
  takes. Only while the gun is still turning do they fly along it. The rule
  needs only the gun's angles, so the server, a bot's seat
  (`TurretSeats.drive`) and the page's predicted rounds
  (`src/runtime/predicted-shots.js`) agree. The AI's own reach test
  (src/ai/classic/vehicles) reads the view's `PitchLimits` up positive and is
  kept as the original has it.

| Turret | Eye (aim frame) | Chase camera: pivot, offset, tilt | View pitch | Yaw | Gun pitch (widened), gun yaw |
| --- | --- | --- | --- | --- | --- |
| `com_weap_gunturret` | 0 1.5 1 | 0 1.5 −2, 0 0.5 1.5, 5 degrees | 45 up, 45 down | free | 45 up, 45 down; ±5 |
| `tat_bldg_barge_turret` | 0 1.5 1 | 0 1.5 −2, 0 0.5 1.5, 5 degrees | 45 up, 45 down | ±70 | 45 up, 45 down; ±70 |
| `bes_bldg_AAA_turret` | 0 6.2 −8.3 | 0 8 5, 0 −3 10, 0 | 30 up, 20 down | free | 30 up (20 written), 30 down; none |
| `geo_bldg_geoturret` | 0 2.5 0 | 0 0 −2, 0 3.2 0, 0 | 45 up, 15 down | free | 45 up, 45 down; ±30 |
| `hoth_bldg_hoth_turret` | 0 0 0 (the drum, 3.6 m up) | the eye, 0 1.2 4, 0 | 10 up, 10 down | free | 45 up, 45 down; ±30 |
| `hoth_bldg_dish_turret` | 0 1.5 0 | the eye, 0 2.7 3, 0 | 10 up, 25 down | free | 25 up, 25 down (10 written); none |
| `yav_bldg_tower_turret` | 0 3 0.5 (the drum, 14.1 m up) | the eye, 0 −1 3.5, 5 degrees | 10 up, 35 down | free | 35 up, 35 down (10 written); ±5 |

The dish turret's high chase camera keeps the turret below the bottom of the
view.

`tests/turret-view.test.js` covers the rules, and for every imported class
the camera's place and rounds meeting the aim point in both views.
`tools/turret-probe.mjs` shoots each class's views; `tools/turret-aim-probe.mjs`
fires each across a wall at 10 m and 100 m, level and at the lowest and
highest pitch its view takes, and measures the rounds against the page's
reticle.

## Data used

From the prepared data (`sides/*/units.json`, `maps/*/props.json`,
`common/effects.json`, `audio/*/sound.json`): `com_item_healthrecharge`
(ActiveRotateNode `pH_tester_arm10`/`pH_tester_arm6`, ActiveSpinNode
`Equipment_Operator_Arm_Socket`, PowerupSound `health_recharge`),
`com_item_weaponrecharge` (IdleWobbleNode `gonk1`, PowerupSound
`com_item_gonkrecharge`), `all_weap_inf_fusioncutter` (MuzzleFlashEffect
`repairkit`, FireLoopSound `com_weap_inf_fusioncutter_fire`). The droid ODFs
name no serve effect; the gain effects are the soldier's, as for power-ups.

Tests: `tests/live-feedback.test.js`; the turrets' effects and sounds in
`tests/vehicle-effects.test.js`, `tests/vehicle-sounds.test.js` and
`tests/test_vehicle_fx.py`.

## Placed power-ups

A map can place a power-up item (`powerupitem`) as it places anything else. The
stock maps place one: Yavin 4: Arena's `com_item_powerup_health100` (Team 0, at
9.6, -46.0, 72.4; 75 health to a person or a droid, `LifeSpan` 30). The
original creates every world instance through its class, so it is an ordinary
item from the battle's start: it falls under its `Gravity` and settles, anyone
touching it takes it (src/items.js), and it lasts its class's `LifeSpan` like
any other. The item update compares its age with the class's `LifeSpan` (fading
over its last half second); nothing exempts a placed one, so an untouched one
is gone 30 s into the battle.

- `tools/zero_placed_items.py` finds them (base `powerupitem`) and
  `tools/extract.py` `write_props` writes each into `props.json`'s `items`
  (class, team, the instance's matrix and the class's item settings, as a
  dispenser's item is written), its model with the pieces' in
  `props_extras.bin`.
- `src/placed-items.js` spawns them in the world as it loads (the world
  worker, the dedicated server, a Node check), never on a page. Each
  travels as an item record like a dropped one
  (`src/runtime/network-entity-items.js`): its definition is the prepared
  object, which the combat registry names by id.
- `tests/placed-items.test.js` and `tests/test_placed_items.py`;
  `tools/placed-items-check.mjs` runs one in a headless match: placed and
  settled, a record, taken by a hurt network human (120 to 195 health of
  300), and gone at 30 s untouched.

## Animated props and the sarlacc

An animated prop (`animatedprop`: Tatooine: Dune Sea's sarlacc tentacles,
Jabba's Palace's Jabba, rancor, torture droid and trapdoor, Cloud City's
carbon-freeze forklift) runs as the original's animated prop does, in
`src/props.js` `AnimatedProp`. Doors (`door`, the palace cell doors) keep their
own rule: open while anyone is in reach, the Open clip run both ways.

What its class gives (`tools/extract.py` `animated_prop_animation`):

- `Animation`, up to five: the clip slots. The first is its idle.
- `AnimationTrigger "<action> <node> <reach> [sound]"`, up to three: a body
  within reach of the node plays the action's slot once (an action that
  names no slot never plays).
- `AttachTrigger` (hash 72306628) `"<node> <reach> <action> <clip> <window>
  <camera node> <word> [sound]"`, up to three: the grab. The camera node
  and the word are read and left unused.
- `IdleDelay` (ff701082, default 0): seconds the idle holds its last frame
  between loops. `KillSoldierSound` (f949d898): the swallow's sound. Flag
  0550a3b2: a seized player's view turns to his death camera at once.
  Flag 6b014234: the idle starts at its first frame (every other prop's
  starts at a random one), and the update stops in some network states the
  port doesn't model.

What it does each update:

- **Idle:** the first slot loops, holding `IdleDelay` at its end. A clip's
  length is its frames at 30 a second.
- **Triggers:** only idling, holding no one, and with the idle's weight (below)
  at least 0.5, each trigger in turn tests its node: any body in the soldier
  list (living, or dead and not yet gone; not seated or held) whose collision
  sphere's box meets the box `reach` round the node (per axis). The first that
  finds one plays its action once (time and weight back to 0) and its sound.
- **The pose eases in:** each update moves the pose the weight of the way from
  where it stands to the clip's (all of it once the weight reaches 1), and the
  weight then rises by the step: a second a second into an action, a fifth back
  into the idle. So an action blends in over about a second, the idle over
  five, and the next trigger waits 2.5 s after an action. The port does this
  per 60 Hz world step; a page's frame takes the share as many steps would.
- **The grab:** while the attach trigger's action plays, up to `window` seconds
  in, the attach node (on an animated bone: the tentacle's `hp_active`, on
  `sarlacc_bone4`) takes the first body found within its reach, if he lives and
  is a soldier (droidekas are not). Seized, he goes from the soldier list to
  the riders' list (rounds still reach him, regions and posts no longer count
  him, blasts hurt him without a push), out of his own control in the soldier's
  pilot state. Each update his root bone (`bone_root`, as his clip poses it) is
  set on the node, turned with it: his matrix is the inverse of the root bone's
  times the node's. His clip is the attach trigger's
  (`human_rifle_thrown_flail`), looped from the seizure. A local human leaves
  first person, and with flag 0550a3b2 his view turns to his death camera now;
  the flag travels to clients.
- **The end:** when the action has run its length, the one held is killed with
  an empty damage record (no one is credited; his death sound plays), his death
  runs at once and his body is deleted, and `KillSoldierSound` plays at the
  prop. It idles again.

Cloud City's carbon-freeze forklift (`bes2_carbon_forklift`, one, in the
carbon-freezing chamber) has no grab: `Animation basepose` (no such clip in its
bank, so it holds whatever pose it is in) and `activate` (498 frames, 16.6 s:
the claws on `bone_root` come down from 10.7 m to 5 m, then the casing on
`bone_casing` rises out of the pit 13 m), `AnimationTrigger "activate
hp_activate 0.5"`, `IdleDelay 10`, flag 6b014234. `hp_activate` is 1 m over the
pit's floor, under the claws' column (two 1 m cylinders on `bone_root`,
reaching down to 5.4 m over the floor), and the pit is Cloud City's
`deathregion` (a 7.3 m cylinder whose top is 0.6 m over the floor). A living
soldier can't stand there, but a body counts: whoever falls into the pit dies,
his body lies at the bottom (a death region deletes nothing), and the forklift
runs its clip, the carbonite casing coming up as the claws come down to it.
`tests/map-interactives-browser.mjs` drops the player into the pit and draws
it.

Tatooine: Dune Sea's pit (`tat1_bldg_sarlaccpit`) hangs five tentacles on
`hp_tentacle1` to `5` (yaw-only hardpoints, taken with the prop's matrix). Each
tentacle has three hotspots, 8.8, 12.5 and 16.3 m out from its root at 5.5 m
up, each 5 m of reach, and grabs from `hp_active` within 1.5, 1.75 and 2 m, up
to 2.5 s into `grab`, `grab1` and `grab2` (90 frames, 3 s each). The rancor
(`tat3_bldg_rancor`) grabs the same way: `hp_active` within 1.5 m, 2.5 s into
`grab`, after a soldier comes within 2 m of `hp_hotspot`. Every battle of the
Dune Sea also names the pit's region `Sarlac01` with `AddDeathRegion`: a
cylinder of 8.43 m radius reaching 6 m up and down from the pit's centre kills
whoever walks or falls in, no one credited. Its `SIZE` is 2.14 x 6.06 x 8.16;
building the region sets a cylinder's radius to the length of its x and z, and
the region test reads that (docs/formats.md "Regions", `src/regions.js`).

The tentacle's mesh is one soft-skinned segment of 308 vertices over nine
bones (docs/formats.md "Models"), each vertex blending three. The port
draws it so (`skinJoints`, `skinWeights` in `props.json`, three a vertex);
split by each triangle's first bone, its pieces had come up to 3.7 m apart
at the bends.

How the port runs it:

- The world (`LiveObjects.update`, every unit as `bodies`) runs triggers,
  grabs and swallows. A held unit's `held` names the prop; `Unit.tick` and
  the player's own step leave him be, his hit shapes are his posed body in
  the frame he hangs in (`AnimatedProp.heldShapes`), and the death regions,
  the soldier push, posts, droids and mines leave him out. His weapons wait
  (`PlayerCombat.mounted`).
- A creature that grabs is the world's alone. Its state (clip slot, the
  tick and time it started, whom it holds and since when) travels as world
  record schema 9 (identity 400000 + its index, `src/runtime/entity-world-
  state.js`), taken at once when it changes (`src/runtime/snapshot-records.
  js`). A page follows it (`AnimatedProp.adopt`,
  `follow`): the clip at the world's time for the tick it draws, which is
  fractional, so the tentacle and whom it holds move smoothly at any frame
  rate, the page's own soldier included (not predicted while held), and the
  same for every other client online. Until the first record a page idles
  the creature itself. Doors and props that don't grab still step on every
  host from the soldiers it has.
- Held, a body is drawn in his clip (`SoldierAnimator.gunner` at the hold's
  time) with his actor's root on the node's frame (`Unit.present`, the
  player's own in `src/game.js`); the seized player's death camera starts
  (`startDeathCamera`) and first person goes off.

`tests/animated-props.test.js` covers the triggers, the reach, the eased
pose, `IdleDelay`, the grab, the swallow and the record;
`tests/test_attachments_export.py` the class keys and the soft skin;
`tests/sarlacc-browser.mjs` (Vite and the extracted Dune Sea) puts the
player and a bot at two tentacles' hotspots and a bot in the pit, and checks
the seizure, the page drawing them where the world holds them, the death
camera, the swallow and the region.

## Ewok log traps

Endor's two log traps (`end_weap_treesmash`, ODF base `trap`, placed with
Team 0 on the west side between the Camp and the Village) run as the
original's trap does, in `src/traps.js` `Trap`. Two logs hang from ropes
either side of a clearing (`bone_r_tree`, `bone_l_tree`, a soft-skinned rope
mesh); cocked they are hauled up 23 m out and 11 m high, and the swing brings
them together over the trap's origin.

The class's keys are stored as name hashes; each name was matched to its FNV
hash (`tools/zero_odf.py` `TRAP`), and the values the stock class sets agree
with the names:

| Key (hash) | Stock value | Default |
| --- | --- | --- |
| TriggerAnimation (f7a5d75b) | `trigger` (161 frames) | none |
| BuildAnimation (2fd8a07b) | `reset` (141 frames) | none |
| BuildPoint (619005ab) | `hp_active` (a hardpoint, read as its matrix) | |
| BuildModelOdf (138b5187) | `com_prop_buildzone` | |
| TriggerTeam (abcb950a) | `imp` (a side: all 1, imp 2, rep 3, cis 4, by CRC of the name) | 0 |
| ResetTeam (2e4c2b79) | `all` (a side, the same) | 0 |
| TriggerNode (58fe8213) | | none: the origin |
| TriggerOffset (fe7c5230) | | 0 0 0 |
| TriggerRadius (c461d811) | | 3.0 |
| MinEnemyRadius (479e5b63) | 4.0 | 0 |
| DeathTime (b4c6360c) | 0.666 | -1 (never) |
| DeathEffect (f138015c) | `explosion` (an effect) | |
| DeathEffectOffset (29824727) | 0 7.5 0 | |
| TriggerSound (836ff8c2) | | none |
| TriggerCollision (146ffe85) | | 0 |
| BuiltCollision (06b4bbb5) | 1 | 0 |
| HideUnbuiltModel (0d689977) | | 0 |

Two float keys (42e950ab and dc6a93e6, both default -1) switch the trigger
test from a box to a swept test along the trap's frame when positive; the
stock trap leaves them off, and the port reads neither. A flag (3fd818d0)
turns on a contact kill of TriggerTeam's things; it is off in the stock
class.

What it does:

- **Placed:** placing the trap makes the build pad (BuildModelOdf's class) at
  BuildPoint, its height set to the ground's, and moves the build hologram
  (UnbuiltHoloOdf, `com_holo_trap`) onto the pad's `hp_hologram`. The trap's
  own sphere becomes the pad's, so repair locks, the AI and its aim go to the
  pad, 28 m from the logs. As every class with an UnbuiltGeometryName it
  starts unbuilt (0 health), posed at the end of TriggerAnimation, the logs
  at rest.
- **Its team:** while its team is 0 and ResetTeam is set, the update gives it
  the team whose side is ResetTeam: the Alliance on Endor. So repair weapons
  that serve friends build it: the Rebel pilot's fusion cutter, and the
  Ewoks' tool (`ewk_weap_inf_tool`, BuildingBuild 15, TargetFriendly) since
  the Ewoks are the Alliance's friends. The Empire's cutters (TargetEnemy 0)
  cannot.
- **Built:** health raised to MaxHealth while unbuilt makes it built: the
  built geometry shows, the pad is hidden, and BuildAnimation plays once from
  its start; then it stands at TriggerAnimation's first frame, cocked. The
  base class's rebuild is not called.
- **Set off:** cocked and not being hauled up, each update queries the box
  TriggerRadius round its trigger point for soldiers and vehicles (at most 5
  found; a body's collision sphere's box against the box, per axis). The
  first whose sphere is larger than MinEnemyRadius (`> 4`) and whose team's
  side is TriggerTeam sets it off: TriggerAnimation plays from its start,
  TriggerSound at the point. 4 m passes an AT-ST (its model's sphere is
  5.53 m) and stops soldiers and speeder bikes (1.94 m).
- **Shot down:** while built it takes damage as a building; at 0 health the
  base health change clears its alive flag and the trap's override then sets
  it off instead of destroying it. It falls once it is cocked (a trap shot
  while being hauled up falls when that ends).
- **The crush:** in the update in which the swing's time passes DeathTime,
  the same box query: every thing in it whose sphere is larger than
  MinEnemyRadius, whatever its side, takes an empty damage record (no one
  credited), dies, and is deleted at once: no death animation and no wreck.
  If anything was, DeathEffect plays at DeathEffectOffset in the trap's
  frame, where the logs meet.
- **After:** when the swing has played (161 frames at 30 a second, 5.37 s)
  it is unbuilt again (0 health), the pad shown; ten seconds later the pad's
  collision box (sized from the pad's bounds) is turned on.

How the port runs it:

- `tools/zero_trap.py` writes `props.json`'s `trap` for each instance (the
  keys above, the pad and its hologram mount); `tools/extract.py` makes
  `trap` a live base, so the traps are not baked into the world, and exports
  both clips and the DeathEffect.
- `src/traps.js` `Trap` (a `Building` from `src/props.js`) keeps the
  original's order per world tick (`LiveObjects.update`): its phase
  (`down`, `reset`, `armed`, `swing`), the tick it began, seconds into it.
  Vehicles are `live.vehicleTargets`; a vehicle's sphere is its
  `blastSphere` (its model's). A crushed hull gets `setHealth(0)` with no
  attacker and `dying = 0`: it is gone at once, its crew dies uncredited.
  The DeathEffect goes through `presentEffect`, so it reaches the pages.
  `src/game.js` gives the traps their sides from the battle's teams (the
  prefix of each team's classes) and their team from ResetTeam.
- The pad stands on the world's collision under BuildPoint
  (`padOnGround`); the hologram is the build hologram, shown to soldiers
  carrying a repair weapon, as a turret pad's.
- The traps are in `live.buildings`: repair locks, BuildingBuild ticks,
  blasts and the Classic AI's repair jobs (`src/ai/services.js`) find them
  as any building. Their state, health and team travel as schema 7; their
  phase and its start tick as world schema 10 (identity 500000 + index,
  `src/runtime/entity-world-state.js`), taken at once when it changes. A page
  draws the clip of its phase at the world's time for the tick it draws
  (`LiveObjects.presentAnimated`).
- Not done: the logs' collision (capsules on the swinging bones; the trap
  has none, so rounds pass through and only blasts at the pad hurt it, the
  pad's sphere standing in for the built trap's collision sphere, which is
  not known), the pad's collision box, and the swept trigger test the
  stock class does not use.

`tests/traps.test.js` covers the build, the haul, the trigger rules, the
crush, the swing's end, being shot down and the record;
`tools/trap-probe.mjs` walks an AT-ST into a trap on the extracted Endor
(and `--battle` counts the traps' builds, swings and kills in Classic AI
battles); `tests/log-trap-browser.mjs` (Vite and Endor) checks the page drawing the
swing, the walker gone and the effect.
