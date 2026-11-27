# Battlefront II hero powers

The 2005 game's Force powers and aura abilities run in the fixed 60 Hz
simulation (`rust/sim/src/combat/powers/`). Only a 2005 battle installs them;
a 2004 battle never reaches this code. Melee combos, blocking, deflection and
the thrown lightsaber are described in [melee.md](melee.md).

## What is implemented

Every number comes from the player's prepared weapon records
(`sides/<side>/units.json`), read once when the battle starts
(`powers::def::from_weapon`):

- **Emitters with a cone** (Force push, Force pull, Force lightning): a
  `cannon` weapon whose ordnance is an `emitterordnance` with a positive
  `ConeLength`.
- **Emitters with a radius** (the aura abilities): an `emitterordnance` with
  a positive `Radius` and timed `BuffOffense*`, `BuffDefense*` or
  `BuffHealth*` values.
- **Held area weapons** (Force choke): an `areaeffectweapon` with
  `FireType Hold`, `EffectType Choke` and `AreaType Ray`.

Only weapons on the offhand trigger (`channel` 1) are powers. An emitter on
the primary trigger, such as a flamethrower, keeps firing through the gun
code. A record of any other shape is not a power and stays with the weapon
code it already had; a chaining emitter or another area form is refused when
the battle starts rather than played wrongly.

### The trigger

The offhand trigger of each life drives a small state machine
(`powers::cast`), stepped once per tick. As in the 2005 game, every power of
the channel runs its own phase each tick, selected or not, and all of them
read the one trigger:

- The selected power starts while the trigger is down: a wind-up of
  `InitialSalvoDelay`. An emitter casts when the wind-up ends, even if the
  trigger was released, then waits `ShotDelay` before it can cast again. With
  `TriggerSingle 0` (such as Force lightning), holding the trigger casts again
  after each recovery, and resumes by itself once stamina exhaustion and
  overheating clear. A `TriggerSingle 1` emitter spends the trigger as it
  starts, and a held `TriggerSingle 1` area weapon spends it as it lets go
  with the trigger still down; a spent trigger starts nothing until it is
  released and pressed again.
- Changing the selected power (the wheel, or the next secondary key) does
  not stop a held area weapon: a choke goes on holding and hurting its target
  while the newly selected power starts from the trigger that is still down.
  Force choke, then the wheel, then Force lightning (Count Dooku and the
  Emperor) holds the target in the choke while the lightning strikes it every
  0.6 s. The caster shows the selected power's pose: the lightning's while it
  is selected, and the hold again once the wheel comes back to the choke and
  the lightning's cast is done. An emitter that is still winding up when the wheel leaves it drops
  that cast.
- An emitter pays `EnergyDrain` when it casts; a held area weapon pays it on
  the press. Payment goes through the soldier's stamina latch, the same rule as
  rolls and combo transitions: one overdraw is allowed while the latch is
  clear, and a latched soldier cannot cast. An emitter that also authors
  `HeatPerShot` accumulates weapon heat on each cast and cannot fire while
  overheated until its weapon cools below `HeatThreshold`. A weapon that
  spends rounds instead of energy has the record's allowance for the life.
- A held area weapon acquires its targets when the wind-up ends, if the
  trigger is still held, and keeps them while it stays held. While it holds
  and is the selected power, its `EnergyDrainRate` replaces the soldier's
  ordinary recovery and its `ThrustFactor` scales the caster's own movement;
  once another power is selected the hold goes on without either. It lets go
  when the trigger is released, the soldier is exhausted, or no target is
  left.
- Death, a seat, being thrown or being held by a choke stops any wind-up or
  hold.

### Who a power reaches

- **Cones** take bodies strictly closer than `ConeLength` whose bearing is
  inside half of `ConeAngle`; within 2.5 m any body less than about 84 degrees
  off the axis counts. Bodies are taken nearest first, a body behind static
  geometry is skipped, and at most `MaxTargets` are struck. `AffectFriends`
  and `AffectEnemies` choose relations (both default to on; a neutral body
  counts as a friend).
- **Aimed rays** take bodies within `AreaRange` along the aim, inside a wedge
  of `AreaRadius` degrees either side, and within half of `AreaHeight` of the
  aim plane, measured to the body's surface. They prefer the soldier's locked
  target, then the nearest, skip bodies behind static geometry, keep at most
  `MaxTargets` (never more than four) and check every third of a second that
  each one is still inside.
- **Auras** take every accepted soldier within `Radius` of the caster. With
  `ApplyOnOwnerIfOnOthers` the caster shares the effect when it reached
  anyone.

### What a power does

- **Damage** goes through the battle's per-target damage service, so team
  damage, death, credit and publication follow the same rules as any other
  hit. A cone deals `Damage` to each target (`MaxDamage` at full charge,
  shared among the targets but never below `Damage`), scaled by the target's
  health category. A held choke deals `EffectDamage` per second.
- **Push and pull.** A positive `Push` spreads from the caster at full strength
  to half the cone length and fades to nothing at twice the length. A negative
  `Push` pushes from a point 1.1 cone lengths from the caster past the target,
  so it draws a far target harder than a near one. The push direction gains
  an upward 0.6 before scaling and replaces the soldier's velocity. A soldier
  on the ground (or rolling) is thrown when it rises faster than 3 m/s or moves
  faster than 8 m/s; one already in the air needs a push stronger than 5, one
  on a jet stronger than 8. A soldier already thrown, seated or held by
  something else is not pushed.
- **After a throw.** A thrown soldier flies under drag and gravity, with no
  spin of the world heading. A class that recovers from tumbles flips upright
  in the air once half a second has passed. Touching down at least 0.3 s after
  the throw ends in a tumble along the ground when the horizontal speed is
  high, sometimes in one when it is middling, and otherwise in a slide. The
  tumble and the slide each brake the soldier and end by speed or by time, then
  the soldier stands up: movement, turning, pitch and fire are locked for as
  long as the stand-up clip lasts. While thrown, tumbling or sliding the mouse
  turns at the class's tumble rate, and the drawn view turns at the rate the
  simulation uses at every frame rate. Where a slide ends with no room to
  stand (a low roof, the underside of a stair) there is no stand-up: control
  returns in a crouch, and the soldier stands when there is room.
- **A guard** facing the caster within its deflect angle stops a cone
  (push, pull, lightning) as it stops a blade; the caster still pays. A choke
  is not stopped by a guard.
- **A choke** holds a standing, crouching or sprinting target: it cannot act
  and turns toward the one holding it, until half a second after the last
  tick that held it. A rolling or airborne target is hurt but not held.
- **Auras** give each accepted soldier timed modifiers. A new modifier
  replaces a running one only when it lasts longer. `BuffOffenseMult`
  multiplies the damage the soldier deals, `BuffDefenseMult` the damage it
  takes, and `BuffHealthRate` restores health per second, up to its maximum.
  The records name no damage source to leave out, so both multipliers apply
  to every hit through the battle's damage path: blades, bolts, explosions
  and the powers themselves. Death clears them.

### Team damage

Powers use the battle's team damage rule for every hit, as other weapons do:
an enemy always takes the damage, a bot never hurts a human teammate, and
otherwise a teammate is hurt only while team damage is on. The 2005 game
keeps team damage on by default, offline and on a server. Whether a power can
reach a teammate at all is the record's: a cone with `AffectFriends` (push,
and lightning by default) reaches friends, pull does not, an aura reaches only
friends, and an aimed ray takes whoever is first along the aim. A push is not
damage: it moves the teammates it reaches whatever the team damage setting.

### The on-foot target lock

The lock button picks the enemy best aligned with the aim within reach (a
cone of 30 degrees either side), in sight; a second press lets it go. The
reach is the selected power's `LockOnRange`, or else the longest
`LockOnRange` among all the character's weapons, so a character without
powers locks too. The lock lapses when the target dies, moves past 1.25 times
the range or stays out of sight for a second. While locked, a cone power
aims at the locked target. An aimed ray (a choke) keeps the soldier's own
aim; among the bodies inside its wedge it takes the locked target first. The
cone, the keep range, the second out of sight and the reach without a
selected power are provisional until a capture measures the 2005 game's own.

### Bots

Bots follow the 2005 game's weapon and power gating:

- **Zero-damage cones and held area weapons** (Force push, Force pull, Force
  choke) are never cast by bots, even if their offhand trigger is pressed or
  held.
- **Damaging cones** (such as Force lightning) are cast only when the battle's
  weapon AI selects that offhand weapon and drives its trigger (detecting each
  rising edge from the bot life's secondary-channel held state and keeping it
  held across repeats). The weapon's default effective range reaches 60 m, so
  the weapon AI selects lightning and holds its trigger against targets
  between 4 m and 60 m, while the lightning cone is 15 m long. A bot therefore
  neither starts nor keeps holding a damaging cone unless an enemy it can see
  stands inside that cone's reach and angle as it aims; a cast already wound
  up still lands. This limit follows how the 2005 game is seen to play; the
  2005 program's own weapon rules, as far as they are known, contain no such
  test. Because lightning affects friends by default with offline team damage
  on, casts near allies can injure teammates.
- **Auras** are checked every 12 ticks (five times a second) while idle. A bot
  casts an aura only when it is in combat, can pay the energy cost without
  overdrawing its stamina latch, and has at least three friendly or neutral
  soldiers (including itself) within the aura's radius.

## How a battle uses it

`powers::install_in` builds a kit for every soldier class from its unit
record: its powers, the explosion its blade can set off, and the reach of its
lock. A class with none of these has no kit. Once per tick the soldier phase
calls `powers::forward_in` for each life with a kit, with the life's offhand
trigger, lock button and stamina account, and then `powers::finish_tick_in`
once to run the timers and health regeneration. `powers::held_in` tells the
soldier phase which lives a choke holds, and a `PowerHost` supplies what only
the soldier phase knows (a roll, a guard) and wraps each hit for its journal.
`Powers::log` keeps the latest power events for presentation and
measurement, and `powers::effects_in` the modifiers and holds on each life.
`powers::reserve_in` grows the per-life storage when the battle's unit slots
grow; whatever is running keeps running.

The timed modifiers enter at the damage path's game family dispatch
(`combat::damage_with`): a 2005 battle scales the amounts by the attacker's
offence and the victim's defence and then runs the released damage body; a
battle of any other family runs the released body alone. The received
modifiers stay in the world while a power pass runs, so a power's own hits
are scaled like any other.

A local Battlefront II battle installs the powers when it admits its soldier
classes; a battle whose records the powers refuse plays without them. Each
tick, after every soldier moved and before the contact journal closes, it
forwards every life's selected offhand power, its press and hold, and the
on-foot lock button (a latched press, like the other taps). An offhand power
never also fires through the gun code. A choked soldier gets no movement or
action input and turns toward the one holding it. Team damage follows the
battle's friendly fire setting, which starts on unless the player turned it
off.

## Fixed capacities

- At most four powers per class, 32 candidate bodies per emission or
  acquisition, four held targets, and 64 logged events between readers.
- Per-life rows and received effects are sized for the battle's unit slots
  when it starts and grow with them, between ticks; nothing is allocated per
  tick.

## Planned

- Presentation of the lock bracket in the battle screens.
- Replication of the thrown state (a pushed soldier's tumble, a thrown blade)
  and of the lock state to remote players. The cast pose and the choke that
  holds a soldier already travel in its combat display (world schema 0x0205,
  [network-contract.md](network-contract.md)).
- Power effects and sounds for remote players: the rings, arcs and impacts and
  the power sounds are drawn from the local battle's power log, which only
  Instant Action hands its client.
