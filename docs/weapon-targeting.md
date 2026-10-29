# Weapon cursors, launcher locks and salvos

How the port draws weapon cursors, acquires and keeps launcher locks, and
schedules salvos, following the original game's behavior. No retail code or
assets are included here.

## Original behavior

- A textured reticle with positive class `LockOffAngle` draws **eight radial
  ticks at the aiming anchor**, not a circle at the target. Each tick extends
  five interface units. Radii are `width * angleRadians * zoom / horizontalFov`
  and `height * angleRadians * zoom / (horizontalFov * .75)`.
- The target diamond is drawn only while the launcher is acquiring a lock on a
  target. Its half-size is `32 - 24 * progress`; alpha is
  `floor(15 + 240 * progress²)`. Target projection is separate from the
  aiming anchor. The reticle texture keeps its original pixel size in
  interface units.
- `LockOnRange`, `LockOnAngle` and target masks belong to the base weapon
  class. Their presence does not grant launcher behavior. Stock rifles and
  pistols contain range and lock-time fields too.
- Missile launchers acquire a target automatically, advance the timer without
  a held trigger, and reset progress on target/subtarget changes. Completed
  acquisition gates the target passed into a projectile.
- `ClearLockOnFire` defaults to true, `LockTime` to zero. The lock is cleared
  when leaving the firing state when configured. A non-positive lock duration
  uses `MaxPressedTime` as its fallback.
- A completed lock is retained in `LockOffAngle`; before completion the
  acquisition cone is used and candidates are compared. A completed valid
  target receives score zero, preventing another candidate from stealing it.
  Range eligibility uses distance to the target bounds, not merely axial
  distance. Both retention and acquisition perform visibility checks and
  target-mask tests.
- A weapon initializes its salvo count and initial delay; delayed salvos
  advance each update, emit `ShotsPerSalvo` rounds, spend ammo and add heat
  per round, and add spread per salvo. Deselecting cancels pending salvos.
  `SalvoCount` is not a synonym for `ShotsPerSalvo`.
- A guided missile recomputes its desired direction from the target point
  each update, with authored turning and waver ramp. If its target reference
  is lost it keeps the last desired direction.
- Every candidate is scored with one cone test: the target's point is the
  centre of its bounding box, its radius half that box's diagonal. A candidate
  passes when it lies ahead, within the range plus that radius, and its
  distance from the view line is at most `t * angle + radius` (t its distance
  along the line); the score is `16 d^2 + t`. `LockOnAngle` is stored in
  radians; `AutoAimSize` writes the same field as `AutoAimSize * 0.072`, so
  the later of the two keys wins. While the channel's trigger is held, a not
  yet locked current target is kept in `LockOnAngle + 45` degrees.

## The lock cone (port)

The radius term means the original's launcher takes a hull well off the
reticle at short range. Measured headless on Rhen Var Harbor with the ARC
trooper's missile launcher against an AAT (bounding radius 6.66 m): at
15 m it locked with the reticle 30 degrees off the AAT's centre, 8 degrees
clear of its hull; at 30 m, 15 degrees; at 120 m, 5 degrees. The port
requires the hull itself inside the cone. `lockSpan` measures the smallest
angle between the reticle line and any point of the target: a vehicle is
its collision box in its own frame (0 when the line runs into it, else the
nearest direction along its edges), anything else its bounding sphere.
Acquisition needs that angle within `LockOnAngle`, a completed lock is kept
within `LockOffAngle`, and the nearest point must lie within `LockOnRange`
and be in sight. Among candidates the one nearest the line wins, then the
nearer. With that rule, at 15 m the AAT locks while the reticle is on its
flank (20 degrees off its centre) and not at 30 degrees; at 30 m it locks
at 10 degrees and not at 15. The 45-degree held-trigger widening is not
reproduced. The reticle's colour and the enemy health bar keep the
original's cone (`findReticleTarget`).

The cone runs along the reticle line, from the weapon's origin through the
point under the crosshair: a soldier's `shotDir`, a vehicle seat's
`reticleRay` (through the seat's target, which a chase camera's crosshair
places), a turret's crosshair point. The same `updateWeaponLock` serves
soldiers, vehicle seats and turrets for the player, bots and network
humans. A bot's launcher takes its controller's masked target
(`botLockTarget`); a human's, local or networked, only what his view puts
in the cone, so a pooled body's stale bot target can never lock for him.

## Port changes

`src/weapon-targeting.js` separates missile-launcher capability from the
ordinary reticle's target-colour selection. Infantry, vehicle seats and
manned turrets feed eligible targets into `Weapon.updateLock`. AI weapons
use their controller's masked target instead of acquiring through a human
crosshair. Dead, changed-generation, obscured and out-of-cone targets lose
the lock; switching weapons clears acquisition.

Lock identity, generation, elapsed time and completion are simulation
fields. Shots capture their guidance identity, so later weapon changes
cannot retarget already-fired missiles. The entity protocol's weapon state
carries these fields and pending salvo timing. Rendering never advances
acquisition. It projects the diamond against the current rendered
camera and follows interpolated target poses, including chase view.

The HUD draws the original reticle texture, the eight aiming ticks and the
separate fading diamond. The inferred range-plus-time marker has been
removed. In particular, ordinary rifles, pistols and the EMP cannon do not
receive missile lock diamonds. A tow-cable weapon's range and timing do
not by themselves enable this launcher cursor either. Reticle dimensions
retain their native size if the raster upgrade replaces their source image.

The Rebel torpedo launcher authors `SalvoCount=2`, `ShotsPerSalvo=1`,
`InitialSalvoDelay=.25`, `SalvoDelay=.25`, `ShotDelay=0`, and a two-round
clip. One trigger now emits rockets at .25 and .50 seconds, including when
the trigger is released in between. ARC/Republic, CIS and Imperial
launchers retain their own one-salvo schedules. Salvos preserve shot
patterns, per-round ammunition/heat and per-salvo spread. Zero-delay
bottomless automatic weapons are bounded to one trigger sequence per
update to prevent an infinite loop.

## Checks and limits

### Tank missile gameplay

The fighter tanks' (the Republic IFT-X and the Imperial IFT-T, which
carry the same concussion missile) and the Rebel combat speeder's missile
channels use the shared launcher acquisition and homing path. Their retail
definitions are unguided cannon/bullet weapons with targeting disabled; the
runtime applies an explicit gameplay override when constructing vehicle
seats. Acquisition takes one second within 150 metres, with a six-degree
acquisition cone and ten-degree retention cone, against enemy vehicles.
Projectile speed, damage, ammunition and firing schedules remain authored.
The override also supplies AI target masks, since extracted AI records
otherwise override weapon flags. The Hailfire keeps its existing authored
launcher settings.

A vehicle seat draws both channels' reticles at one anchor, each coloured by
its own weapon's target, as the original game does (docs/formats.md, "Seated");
the lock diamond is the seat's missile launcher's on whichever channel it is,
so it is visible before firing, and the enemy health panel follows channel
0's target.
`tests/vehicle-missile-lock.test.js` checks the overrides, extracted ground
launchers, moving-target guidance, target generation loss, HUD channel
selection, and the vehicle constructor plus player and AI firing paths.
Aircraft retain their existing flight targeting system.

`tests/weapon-lock-salvo.test.js` covers capability, masks, visibility,
acquisition versus retention, generation changes, firing/switching resets,
projectile guidance, partial clips, firing rates and cursor geometry. It also
checks all four retail infantry launcher definitions when local extracts
exist. `tests/weapon-cursor-browser.mjs` exercises the real ARC combat/HUD
paths in chase and first-person views, checks that the diamond projects onto
the target in both, and verifies that a switch to the pistol removes the
marker. `tests/reticle-presentation-browser.mjs` runs with the world in its
worker and 144 Hz presentation: the displayed aim stays aligned in both views
while authoritative packets are pending.

Vehicle targeting still uses the port's existing collision-bounds aim
point. The original game's per-vehicle subtarget selection is not yet
reproduced, so where every vehicle component is targeted is not pixel
equivalent. The bitmap itself is the extracted retail asset; line drawing
uses the port's HUD quad renderer rather than the original D3D line path.
