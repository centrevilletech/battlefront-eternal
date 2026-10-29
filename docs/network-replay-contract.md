# Shared simulation and network replay

Single player, the authoritative server and client prediction must use the same
simulation methods. Fix movement in `Soldier.tick`, vehicle motion in
`Vehicle.tick` / `stepFlight`, and recoil in `PlayerCombat.updateKick`. Do not
copy those rules into a networking adapter. Prediction supplies recorded input,
steps those methods at their normal fixed timestep, and suppresses publication
of predicted collision bodies or authoritative damage.

The entity correction modules only serialize and restore state. Remote entity
interpolation is presentation and must never become the own player's physics
baseline. No networking mode should acquire its own movement constants, weapon
eligibility checks, steering formulas or collision implementation.

## When shared gameplay changes

For every new or changed mutable field, decide whether a subsequent simulation
step reads its previous value. If so, the correction must preserve it exactly,
or preserve a bounded, validated reference that reconstructs it. Add a
continuation test that restores into a different, dirty instance and crosses the
new behavior's transition. Compare the real shared simulation after several
steps, not just encoder/decoder round trips.

Fields assigned before their first read can remain local scratch. Document that
assignment when excluding them. Class, mission and animation constants can stay
out of the wire only while the content prerequisite supplies the same values on
both peers. A new runtime modifier to one of those constants makes it replay
state and needs an explicit correction field or deterministic reconstruction.

Presentation history such as `previous`, `tickFrom`, wheel phase and accumulated
camera smoothing belongs to the client. Restore history to the corrected pose
where needed; do not transmit another process's render history. Preserve
`settled` separately when it differs from the Soldier position, including signed
zero. Preserve active dive, transition and trick history until that operation
finishes.

Authoritative held buttons and merged view settings remain in the own envelope.
A bounded client command ring can lose the acknowledged command during a long
blackout, and a partial command does not reconstruct merged settings. Movement
receipt, executed input and discarded input are different acknowledgements.
Changing their meaning requires updating both replay and transport tests.

Weapon timers and recoil remain float64. A smaller encoding must demonstrate
unchanged firing/reload eligibility on both sides of a tick boundary before it
replaces that representation. New fields require a coherent mandatory protocol
revision when old peers cannot decode or reconstruct them; do not add another
per-feature runtime switch.

## Current correction ownership

| Body | Exact history | Locally reconstructed or retained |
| --- | --- | --- |
| Human Soldier | Position, velocity, ground normal, fuel, aim, dive, state/lock timers, posture, air, grounded; conditional settled and dive source | Matched class/mission/clip constants (the class's `collisionRule` too); overwritten collision scratch, `collisionDamage` included (zeroed at the start of each tick, its contacts' damage taken by the authority only: prediction never applies it); local previous pose and smoothing; AI avoidance is not human replay state; the mount a rider's dead body falls through (`passThrough`, set at his death on a riding animal, cleared at spawn) is read only by a corpse, which nobody predicts |
| Flyer pilot | Float64 motion, targeting and telemetry scalars; position, velocity, orientation; held flight edge state and pilot aim | Previous/tick pose resets to current; transition endpoints/orientation only during takeoff/landing; trick frames only while that trick is active |
| Ground pilot | Position, velocity, orientation, hull filters, pitch/roll rates and suspension support, health/team, alive/sleeping and pilot aim/look state | Matched hull/seat configuration; per-step suspension scratch; local wheel/walk render phase and previous pose |

Mounted weapons, non-pilot seats, turrets and droidekas need their own audited
state. Supporting a ground pilot does not establish those modes' correctness.

A destroyed walker's `dying` clock (its death animation still to play,
docs/vehicles.md "Dying") is lifecycle state outside the pilot replay: the
hull has no pilot by then. Remote views get it in the vehicle record (state
`Dying`, the seconds left in `power` as milliseconds) until its tombstone.

A walker's water clock (`waterClock`, src/vehicle-water.js) is the host's
damage timer: a predicted hull never deals itself damage (its `damage` is a
no-op), so the clock gates nothing in the replay and is not carried. Its
rule and depth (`waterRule`, `waterDepth`) are class configuration.

## Validation

The compact correction suites instantiate the actual `Soldier`, `Vehicle` and
flight methods, restore dirty instances and continue identical inputs. The
replay-inventory test makes new Soldier fields and new vehicle scalar fields
require an explicit classification. It is a review tripwire, not a substitute
for branch/transition coverage.

Run those suites with the single-player movement, collision, flight and vehicle
regressions. For owned content, `tools/own-player-correction-check.mjs` checks a
real infantry class and TIE, and `tools/ground-player-correction-check.mjs` checks
real ground hulls on the selected map. Their reports identify the source commit
and dirty state. These functional checks do not qualify network loss behavior,
render latency, or 64-player capacity.
