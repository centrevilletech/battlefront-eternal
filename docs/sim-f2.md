# Vehicle phase ownership

The F2 opt-in is `?f2=rust` in a headless world, or `BFE_F2=rust` under Node.
It is independent of the older hull and seat experiments and defaults off.

The final F2 boundary covers pads, hulls, flight, rigs, leases, mounted weapons,
tethers and rider writeback. Rust currently owns hull and flight scheduling,
hull leases and stationary turret crews. Hull firing, tethers and continuous
hull rider writeback remain foreign operations.

Rust owns each vehicle's position, previous position, velocity, heading and rates,
support, health, dying clock, water clock, sleeping flag, flight checkpoint,
wheels, tick capture and retained rig. The generated vehicle slab stores hull and
flight state. Retained rigs are keyed by the slab handle and gameplay life.
A respawn keeps the Vehicle and target objects, increments the target generation,
and resets the new life. Retirement invalidates the slab handle before reuse.

JavaScript readers still use Vehicle methods and their existing vectors. The
published fields are the vehicle schema's mirror fields plus compact pose data.
The full rig remains published because foreign hull firing and rider writeback
read its nodes. That output can shrink after those users
move. Full hull imports are construction-only, never a tick input.

Foreign inputs are registry facts and team relations, body flags, unit facts,
AI intents and firing recoil inputs. JS still owns hull weapons and tethers.
Lease lifecycle, look, seat commands and turret weapons are native. These inputs
do not permit writes to physics, flight or lease mirrors. AI intents remain JS-owned. Damage, health,
supply and wake writes become synchronous scalar commands; commands publish the
touched mirror before returning. A stale handle or gameplay life is rejected.

The scheduler order is clamp/time, spawn clocks, latch/exits, ground capture,
flight, ground substeps, dying, all aim passes, all gameplay render passes, then
foreign hull fire, tether and rider work, then native turret crews. Unmanned
turret guns ease at the building lifecycle's existing scheduler point. Render's leg easing and the ground leftover
alpha are gameplay state and must remain. No foreign callback may retain a Rust
borrow or reimport a whole fleet. Native intermediate writes stay pending until
the next foreign hook or command return; only those boundaries publish output.

Boundary counters distinguish F2 export phase calls, lifecycle calls, commands
and debug calls. Generic import bridges keep their own accounting; these totals
are not a census of every call made by the shared simulation instance. Field counts are scalar words actually copied, including list payloads and
rig output, rather than a count of property names. Debug mirror checks compare
published values with `Object.is`, so signed zero and NaN retain their meaning.
They do not install property traps or alter JavaScript intrinsics.

Native seat orchestration lives in `f2/seats` and `f2/live_seats`; firing and tow
work belongs in `f2/firing`.
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
then hull handles with tow facts and recoil inputs. Its layout is independent of
the complete vehicle schema. Generated F2 seat input records carry foreign unit
facts and AI intents separately. Dynamic bodies return after foreign hull rider writeback
so later damage cannot restore a stale body. Mouse samples use scalar commands and preserve
the original per-sample clamp and normalization order. A lifecycle callback
cannot replace another hull's pending input.

Publications contain generated dirty hull and pad rows, changed body matrices
and bounds, compact pose records, weapon locks and life metadata. Hull rows are
compared bitwise with the last publication; unchanged rows are omitted, including
across reentrant commands. Native rig matrices are copied without JS rig updates. The last records
keep gameplay generations distinct from slab generations and initialize fixed
flight configuration on a newly constructed variant. A spawn publishes its
render before latching, so boarding reads the new life's frame. Commands reject
both retired slab handles and old gameplay lives. Retired references can still
read their final mirror, but cannot target a replacement.

The debug detector runs before and after the phase and checks the owned scalar,
vector, matrix, wheel and flight fields. It excludes the declared foreign weapon and
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

## Native seat orchestration

`vehicles::f2_seats` retains hull seat facts and turret rigs beside the canonical
store. It sequences the existing seat solvers and validates commands against
slab generations and gameplay lives. Commands collect changed hull, turret,
lease and rider references for publication. Released leases retire after the
caller publishes their final state, allowing old emitter objects to keep their
identity. Latch and turret iteration reuse their scratch buffers.

The live F2 scheduler imports foreign unit facts once before native latching.
Generated lease and body records refresh the existing JS facades, including before
synchronous board, exit and death hooks. Scalar commands cover boarding, exits,
moves, displacement, network input, seat cycling, stick reset and destruction.
Lease slab generations and occupant gameplay generations cannot retarget old
references after reuse.

Turret crews retain their aim, rig, weapon and occupant. CrewWorld calls
Weapon::update and ordnance::Cx::fire_round directly, preserving each crew's aim,
body placement and fire order. Shot storage is reused. Random draws still import
the world's existing stream on shots. Bind, sound and fired records use a reserved
ordered journal; native lease emitters preserve beam occupancy across departures.
Building health and rebuild remain JS-owned lifecycle facts. The overlapping props
adapter does not install turret method adapters on this path.

The old seat bridges remain available for their independent switches. F2 never
routes through them. Full hull rig publication and foreign continuous hull rider
writeback remain until the firing merge and consolidated unit writeback. Native
turret targeting and firing currently duplicate a few mounted-weapon helpers;
those can share an implementation when the two owners merge. Private prediction
hulls clear the authority command owner before replaying input.

The warm F2 census below uses ticks 901 through 1200, seed 1, and the same
export/explicit-copy counters as the earlier hull-only boundary. Values are per
tick; inputs and outputs are scalar words. Imported query and RNG bridges are
outside this counter scope.

| Map | Export calls before | Export calls now | Input words before | Input words now | Output words before | Output words now |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| hot1 | 1.01 | 1.01 | 2314.08 | 3734.63 | 15386.55 | 23903.18 |
| bes1 | 1.06 | 1.0633 | 3077.32 | 4881.07 | 16798.41 | 20468.79 |

Words increased while full native rig matrices, native lease/body mirrors and
the foreign hull rider body handoff coexist. Continuous hull rider writeback
still runs the existing JS rider helpers, including their node queries. Removing
those readers and their rig output belongs with consolidated rider writeback.
These counters do not establish a performance improvement or an allocation
claim for the pre-existing hull physics loaders.
