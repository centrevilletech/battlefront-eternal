# Vehicle phase ownership

The vehicle phase is required in every world worker and dedicated server.
Each battle's simulation instance owns the fleet, seats, weapons and riders.
Comparison gates use recorded results from a frozen engine revision.

Rust owns spawn clocks, hull and flight state, retained rigs, hull leases,
stationary turret crews, mounted weapons, tethers and rider body writeback.
The vehicle slab is canonical. Retained class, animation and geometry handles
are resolved at construction; a hull's substeps reuse native working storage.
Respawn preserves the Vehicle facade, increments its gameplay generation and
resets that life. Slab retirement invalidates old references before slot reuse.

AI intent comes from the native AI phase, which runs earlier in the same world
tick (`rust/sim/src/world/step.rs`). `src/registry.js` assigns the gameplay unit
identities; native unit and battle owners retain authoritative gameplay state,
including building health and rebuilding (`rust/sim/src/battle`). Unit facts enter at phase entry,
before spawn callbacks can issue scalar commands. Logical latching stays in its
original scheduler slot; a command amends only its touched canonical actors. Body changes, registry changes, remote flight keys and weapon modifiers
use retained numeric baselines; only changed records cross. Cache baselines
commit after native latching, so a pre-latch lifecycle callback cannot discard
pending input from another hull. Clock and water samples retain the original
phase-start value through those callbacks.

Synchronous scalar commands cover health, damage, wake, attribution, boarding,
exits, promotion, movement between seats, displacement, network seat input,
disconnect, death unseating, seat cycling and look. They validate both slab and
gameplay generations, publish the touched mirrors and return with JS readers
current. Released lease generations cannot target a replacement occupant.
Private prediction hulls clear their authority owner before replaying input.

## Scheduler and native readers

`f2_step(dt,tick)` preserves Vehicles.update's order: clamp/time, spawn clocks,
crew latching and exits, ground capture, flight, ground substeps, dying, all aim
passes, all gameplay render passes, hull fire and tethers, hull rider writeback,
then stationary turret crews. Render's leg easing and ground leftover alpha are
gameplay state. Dormant server-local Gunner/Driver instances stay inactive.
Unmanned turret easing remains at the building lifecycle's scheduler point.

Hull firing reads canonical leases after latching. Hull and turret weapons share
Weapon.update, round execution and human lock targeting. CrewWorld preserves
each turret crew's aim, body placement and shot order. It calls ordnance directly,
without a JS fire detour. Both native beam-emitter intercepts remain because a
hull emitter's life and a stationary lease's life are different identities.
Random draws still import the world's existing streams on shots. Ordered events
carry bind, sound and fired presentation; state-sensitive hooks stay synchronous.

Native rigs supply seat eye/frame/direction and muzzle geometry. Rider skeletons,
clip bindings and sampling scratch persist per actor life. Rust computes exposed
hit shapes, drop points and rider bodies in their original scheduler slots.
It preserves Unit.tickCount and the existing lazy hit-shape invalidation rules.
JS applies compact reader samples and actor presentation fields. World-phase
readers neither traverse JS rig nodes nor call updateMatrixWorld. Render-facing
scene matrices can still update during presentation, outside this world phase.

## Generated boundary and publication

`f2_init` reserves input and returns a stable generated descriptor. One phase
export updates its output addresses, so ordinary reads require no getter calls.
The phase prefix, foreign records, flags, commands, compact geometry and sparse
publications all use schema-generated layouts. The standard counted-block header
is the shared store ABI. Named flag bits describe the scheduler's pose and flight work.

Generated word masks address retained encoded mirrors. Native publication compares
bits, including signed zero and NaN payloads, then sends changed words only. Hulls
and riders that did not change are omitted. JS retains the encoded row and applies
the generated reader locally. There is no full rig publication or rider-body
handoff. Collision bounds/matrices are published only when changed and use the
shared native collision world's generated layout.

Commands and reentrant hooks publish pending touched rows, never a whole fleet.
A spawn publishes only its new life's frame before boarding can read it. No
foreign callback keeps a Rust borrow across JS. Resuming a callback reads the
canonical slabs rather than restoring an outer snapshot. Lease views refresh
after every seat change, including promotion and destruction in the same phase.

The optional mirror detector compares saved scalars with Object.is and rejects
owned JS writes. It does not install traps or replace intrinsics. Initialization
reserves a memory margin; debug phases also reject unexpected memory growth.
The production boundary reuses arrays, numeric buffers, native working hulls,
collision query storage and pose/targeting scratch. Loaded geometry reserves
wall-cache indices and triangle buffers by polygon identity, so reaching a new
part of the map does not grow query storage. Allocation tests exercise the live
scheduler over synthetic hulls, crews and exposed riders without game files.

Boundary counting is test instrumentation. The running game calls the module's
exports directly and performs no counter updates during publication or sync.
Command bursts and lifecycle hooks add boundary calls; an ordinary tick uses
one f2_step call. The warmed synthetic scheduler tests make zero native allocation
requests and sample no JavaScript object allocations. Live battles still allocate
while decoding foreign unit mirrors and emitting presentation events; those
remaining bytes are measured separately from boundary word counts.

The ownership and publication gates run without game files over synthetic
hulls, crews, turret rebuilds, exits and exposed riders. Mounted-fire and rider
samples are golden results recorded before removal of the JavaScript authority.
The full battle gate compares all three world-hash columns with the frozen
reference for the standing matrix, including player-mounted battles, hero rows
and mounted Hoth crews through death and respawn. Native replays and prediction
checks retain independent coverage of operation order.

Node battle hashes do not exercise a page applying leases or presenting riders.
A desktop release also runs `tools/desktop-world-advance.mjs` against a loaded,
resumed real desktop battle on each performance map. Pass its remote-debugging
port as `CDP_PORT` and use `--seconds 40`; `--minimum-tps` sets a measured rate
floor for the machine and map. The check samples both clocks against wall time,
records player life and pause/load/running flags, and rejects stopped clocks,
lockstep, suspension and page or worker errors. Run interleaved control timings
alongside it: a stopped world must never qualify as a cheaper page frame.

The page's private hull, walker and flight prediction runs in the Rust
`ClientPrediction` owner using the shared simulation cores. Prediction restores
authoritative checkpoints and replays pending commands in a private world;
it does not advance the authoritative fleet. Displayed hulls, gear, riders and
turrets use Rust presentation. JavaScript retains authored resource admission,
replicated lease facades and host input/query transport. AI, unit combat and
building lifecycle are canonical native services in the same World.
