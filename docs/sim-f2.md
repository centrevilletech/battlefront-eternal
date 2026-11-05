# Vehicle phase ownership

The F2 opt-in is `?f2=rust` in a world worker or `BFE_F2=rust` under Node.
It is independent of the older hull and seat experiments and defaults off.
The existing JS implementation remains available for comparison.

Rust owns spawn clocks, hull and flight state, retained rigs, hull leases,
stationary turret crews, mounted weapons, tethers and rider body writeback.
The vehicle slab is canonical. Retained class, animation and geometry handles
are resolved at construction; a hull's substeps reuse native working storage.
Respawn preserves the Vehicle facade, increments its gameplay generation and
resets that life. Slab retirement invalidates old references before slot reuse.

JS owns AI intent until its phase moves, plus the foreign unit registry, team
relations and building health/rebuild lifecycle. Unit facts enter at phase entry,
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
is the shared store ABI. Named flag bits preserve the old pose/flight switches.

Generated word masks address retained encoded mirrors. Native publication compares
bits, including signed zero and NaN payloads, then sends changed words only. Hulls
and riders that did not change are omitted. JS retains the encoded row and applies
the generated reader locally. There is no full rig publication or rider-body
handoff. Collision bounds/matrices are published only when changed and use the
same generated layout for native and fallback query worlds.

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

Boundary counters distinguish phase exports, scalar commands and lifecycle calls.
They count scalar words actually copied, including variable event payloads.
Generic query and RNG bridges keep their existing accounting and are outside
this export/copy census. A command burst or lifecycle hook adds export calls;
ordinary ticks use one f2_step call.

The seed-1 window below is ticks 901 through 1200. The earlier boundary already
had native seats but still published full rigs and handed rider bodies through JS.

| Map | Earlier calls/tick | Current calls/tick | Earlier input words/tick | Current input words/tick | Earlier output words/tick | Current output words/tick |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| hot1 | 1.01 | 1.097 | 3734.63 | 969.99 | 23903.18 | 1790.54 |
| bes1 | 1.063 | 1.480 | 4881.07 | 1213.19 | 20468.79 | 2436.72 |

Most input is still foreign unit facts and intent. Most output is compact rider
samples, body/lease mirrors, weapon state and hull/seat geometry for existing JS
readers. These are explicit transitional readers, not a second simulation owner.
With native collision enabled, flags and changed collider matrices add the
following scalar copies. A copied flag counts as one word in this census.

| Map | Calls/tick | Input words/tick | Output words/tick |
| --- | ---: | ---: | ---: |
| hot1 | 1.097 | 1089.99 | 2129.15 |
| bes1 | 1.480 | 1309.19 | 2814.11 |

Both native-collision windows make zero native allocation requests. The warmed
synthetic JS boundary also has no sampled object allocations. Live JS allocation
sampling still attributes allocations to mirror decoding and presentation, so the strict
whole-battle zero-allocation target is not met. The word census alone does not
establish a CPU improvement; local emulator measurements are reported separately.

The local emulator comparison uses three interleaved 20-second recordings per
variant and map, with seed 1, Classic medium, heroes off and native collision.
All 24 recordings have valid durations and advancing worker ticks. These are
CPU measurements of the frozen `9f2ca4b2` build, before the subsequent renderer
and hero-record merges. Both variants use the same renderer and wire capture;
the F2 phase source is unchanged by those merges. Brackets show the observed
range of the three p50s, rather than a confidence interval.

| Map | Default step p50 [range], ms | Rust step p50 [range], ms | Change | Default/Rust p90, ms | Default/Rust ticks/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| nab2 | 9.47 [9.39–10.58] | 9.56 [9.13–10.01] | +1.0% | 16.81 / 16.18 | 59.95 / 60.06 |
| tat1 | 20.27 [18.56–20.89] | 17.31 [15.26–19.02] | -14.6% | 29.32 / 25.99 | 46.02 / 51.93 |
| bes1 | 14.21 [14.08–16.19] | 14.94 [14.68–15.59] | +5.1% | 23.34 / 22.50 | 58.04 / 58.23 |
| hot1 | 15.35 [14.41–15.66] | 12.15 [11.02–12.24] | -20.9% | 22.63 / 18.91 | 56.80 / 59.60 |

Page CPU p50 changes from 24.62 to 26.69 ms on nab2, 39.00 to 40.19 ms on
tat1, 26.78 to 27.91 ms on bes1 and 26.96 to 25.42 ms on hot1. Thus the
worker improvements on two maps do not establish a general page improvement.
Renderer diagnostics are the same within each variant pair. These recordings
make no GPU performance claim.

The native profile does not establish a solver improvement. The inclusive
`veh_ground` entry rises from 1.281 to 1.705 sampled ms/tick; it includes queries
and posing, and the earlier/current windows are ticks 2215–3459 and 1832–3078.
Full phase time rises from 2.131 to 3.149 sampled ms/tick. Native publication
accounts for 0.372 ms/tick and JS sync for 0.370 ms/tick in the current profile.
Rust leaf work occupies 58.7% of the phase, compared with 54.9% earlier, but now
also includes native crew, firing and rider work. These different scopes and
windows prevent an isolated arithmetic comparison. The smaller boundary has
not removed publication and mirror processing as substantial costs.

The old `seats-sim.js` and `turret-seats-sim.js` adapters remain for their independent
switches. F2 never routes through them. Building lifecycle and ordinary unit AI
remain foreign; their later ports can further reduce the unit fact boundary.
