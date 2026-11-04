# Vehicle phase ownership

The F2 opt-in is `?f2=rust` in a headless world, or `BFE_F2=rust` under Node.
It is independent of the older hull and seat experiments and defaults off.

The final F2 boundary covers pads, hulls, flight, rigs, leases, mounted weapons,
tethers and rider writeback. The first milestone transfers hull and flight
scheduling; seats, firing, tethers and riders are foreign operations until their
remaining steps land.

Rust owns each vehicle's position, previous position, velocity, heading and rates,
support, health, dying clock, water clock, sleeping flag, flight checkpoint,
wheels, tick capture and retained rig. The generated vehicle slab stores hull and
flight state. Retained rigs are keyed by the slab handle and gameplay life.
A respawn keeps the Vehicle and target objects, increments the target generation,
and resets the new life. Retirement invalidates the slab handle before reuse.

JavaScript readers still use Vehicle methods and their existing vectors. The
published fields are the vehicle schema's mirror fields plus compact pose data.
The full rig remains published during this milestone because the unported firing,
boarding and rider code reads its nodes. That output can shrink after those users
move. Full hull imports are construction-only, never a tick input.

Foreign inputs are registry facts and team relations, body flags, frozen crew
input, seat look and lease facts, and firing recoil inputs. JS still owns weapons,
lease lifecycle and tethers. Those are explicit boundary inputs, not permission to
write physics or flight mirrors. AI intents remain JS-owned. Damage, health,
supply and wake writes become synchronous scalar commands; commands publish the
touched mirror before returning. A stale handle or gameplay life is rejected.

The scheduler order is clamp/time, spawn clocks, latch/exits, ground capture,
flight, ground substeps, dying, all aim passes, all gameplay render passes, then
foreign fire, tether and rider work. Render's leg easing and the ground leftover
alpha are gameplay state and must remain. No foreign callback may retain a Rust
borrow or reimport a whole fleet.

Boundary counters distinguish phase calls, lifecycle calls, commands and debug
calls. Field counts are scalar words actually copied, including list payloads and
rig output, rather than a count of property names. Debug mirror checks compare
published values with `Object.is`, so signed zero and NaN retain their meaning.
They do not install property traps or alter JavaScript intrinsics.

Later seats work belongs in `f2/seats`; firing and tow work belongs in `f2/firing`.
The scheduler owns ordering, lifecycle owns birth/retirement, and the integrator
owns the schema, shared publication and game wiring. Changes to those shared
interfaces must land before parallel seats and firing work begins.

## Milestone boundary layout

`f2_init` reserves the input and returns a stable descriptor. `f2_step(dt, tick)`
publishes output addresses in that descriptor, so reading the result requires no
getter exports. Inputs begin with clock, water level and pose/flight flags, then
one post-presence/team pair per pad and an optional spawn occupant. The last two
input words give the foreign block's end and start.

The foreign block contains registry rows, team relations and frozen flight keys,
then hull handles with pending mouse-stick commands, seat lease/input/aim facts,
tow facts and recoil inputs. Its layout is independent of the complete vehicle
schema. Mouse samples accumulate in JS input state; they never write the canonical
stick mirror. They are consumed once before flight, including the original
per-sample clamp and normalization order. Seats and tethers remain foreign state
through their later steps.

Publications contain generated hull and pad rows, changed body matrices and
bounds, compact pose records, weapon locks and life metadata. The last records
keep gameplay generations distinct from slab generations and initialize fixed
flight configuration on a newly constructed variant. A spawn publishes its
render before latching, so boarding reads the new life's frame. Commands reject
both retired slab handles and old gameplay lives. Retired references can still
read their final mirror, but cannot target a replacement.

The debug detector runs before and after the phase and checks the owned scalar,
vector, matrix, wheel and flight fields. It excludes the declared foreign seat and
tether state; attribution is checked by identity. Commands publish current state before returning.
Debug runs also reject memory growth during the phase; initialization reserves a
memory margin. Counters include the whole export rows and variable payloads,
including rig output while the JS firing and rider readers remain.

## Follow-on file ownership

Seats work can add `rust/sim/src/vehicles/module/f2/seats.rs` and
`src/f2-seats.js`; firing/tow work can add
`rust/sim/src/vehicles/module/f2/firing.rs` and `src/f2-firing.js`. Neither should
edit the other's files. The integrator owns `module/f2.rs`, `f2-sim.js`, the
schemas, `vehicle-phase-data.js`, game wiring and regenerated artifacts.

Seats replace the latch callback and the seat part of the foreign block; firing
replaces the JS calls after native posing and the recoil/tow parts of that block.
Both use the same generation-qualified hull references and canonical vehicle
slab. The integrator joins their calls in the existing scheduler order. Step 6
then consolidates synchronous hooks and rider writeback before step 7 removes the
remaining JS world path and measures the default-flip candidate.
