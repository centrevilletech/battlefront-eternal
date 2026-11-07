# Unit phase ownership

`units/f3.rs` owns adopted units, bodies, combat, weapons, factory pools and the
mixed registry. `f3-sim.js` publishes ordinary pooled objects for existing AI,
battle and network readers. Every headless battle uses this owner; the former
unit/combat adapters and their query/environment selections are removed.

## State and identity

Construction imports each complete body once. A running native frame borrows
its slab value without constructing an empty Unit. The slot remains occupied
with the same qualified identity and free-list order; callbacks restore the row
before reading it. Class content, skeletons, clips,
weapon services, collision identities and possible vehicle variants are prepared
before gameplay. Phase input contains intents, trigger edges and declared foreign
facts, never another copy of every body. Explicit commands change only their
qualified participants. Publication contains retained word deltas for changed rows and preserves existing
vectors, including `supply.position === soldier.position` and item-target aliases.
The generated reader skips unchanged body and combat sections, including their
weapon heap spans, individual vectors and optional references. The shared delta
format can name changed words in the retained native cache until acknowledgement,
without copying them through a second values heap or a JavaScript encoded row.
JavaScript decodes the retained native words directly and keeps only a small
mutable dirty mask; generated readers carry its relative word origin. Scalar comparisons preserve
all numeric bits and gather masks in the same order. Descriptor widths are loaded at construction; later barriers
refresh only output cursors, event epochs and result counters. An aimer with
unchanged unit/fleet versions and birth cursor skips that descriptor decode.
Within a changed combat row, unchanged weapons and motion scalars retain their
decoded values. Reload/throw notification latches and deferred muzzle shots have
no foreign gameplay reader and stay entirely in the native combat row. Foreign
controls refresh consumed one-shots because JS can write their next intent
independently of retained publication words. Canonically controlled bots publish
only observed held inputs; consumed movement and trigger latches stay native.
Unit, combat and weapon mirrors use ordinary scalar properties and retain their
pooled body/vector objects. Generated readers update changed groups in place.
Native bot held inputs are an observer facade over the generated retained row;
reading axes, button counts or aim samples its current publication bank. These
fields have no eager per-tick decode. Foreign commands keep writable backing
values, and replacement facades detach from the old bank before rebinding.
Weapon facades still use the same qualified command setters. A weapon getter
decodes its complete retained row once per changed publication, without a WASM
call. Unobserved weapon rows need no JavaScript decode; reading after several
native updates observes the latest complete row. Structural reset initializes
the new arsenal once and preserves its prepared child storage.

The Unit.onMoved import carries the facing, position, velocity, posture and
shape clock that the Classic and Enhanced aimers actually read. Remaining motion
and correction fields arrive through the completed publication. The same barrier
flushes immediate damage/lifecycle changes before foreign callbacks run.
Unrelated completed weapon clocks have no aimer reader and publish together at
the outer barrier. Borrowed movement fields refresh even if separation returns
exactly to their consumed baseline, leaving no native word delta. Character and
seat callbacks flush completed rows too. Mouse look and contact commands reuse consumed combat
encoding; any unread combat delta requires the complete encoder.
Intents are contiguous short rows. Actors with network facts append a separate
generated trigger/rewind row; the foreign header declares that tail's count.
Bots send no absent-edge padding. The phase consumes both spans before any
callback can reuse its input reservation.

Slot generation names the pooled object. Gameplay life names its current spawn;
both qualify commands. Returning a corpse to its class stack preserves registry
membership and object identity. Retirement is a separate operation. Warm factory
releases use the same stack, and its membership bit prevents duplicate returns.
A passive player viewpoint shares canonical geometry and commands without joining
the factory tick list or free stack. Its historical literal-player launch and
self-hit rules remain distinct from a Unit reference.

`Unit.remove` synchronously detaches its foreign Character, clears the canonical
body and returns it to the pool. Drawing cleanup follows in a qualified notice;
an old corpse notice cannot hide a body already respawned by a later phase.
`PlayerCombat.reset` replaces the JS arsenal facade while reusing native weapon,
seen and weapon buffers plus one shared phase scratch. Repair locks and last-attacker attribution retain the
original reset behavior.

## Phase and foreign operations

One owning entry executes units and their map hazards, vehicles and seats, recon
finish, then separation. Native services pass the same borrowed recon authority
and retained query scratch through damage and recursive explosions. F1's later
ordnance and item entries join the same ownership and event machinery.

The AI runs natively in the same tick (rust/sim/src/ai, [sim-ai.md](sim-ai.md)). Its post-movement aimer receives the declared movement facts at the
original per-unit point and returns typed scalar deltas in retained storage.
RNG callback setters bind only the touched weapon or combat stream, including
replacements whose presence remains true. Ordinary ticks do no loadout polling.
Explicit AI sensors observe canonical geometry at the caller's point. Neither
operation imports body state. Character/ticket credit, AI controller transitions
and narrow prop visibility/bookkeeping leaves stay synchronous. Pure shot,
damage, seat, sound and presentation observers run after the outer return through
the shared generated event stream. Epoch and prefix cursors prevent duplicate
notice delivery without getter or clear exports. The reader acknowledges each
prefix before invoking its observer. An observer's later command retains the
unread suffix; nested delivery resumes only after the outer observer returns.

Building geometry, looks, collision bodies and repair keys are prepared once.
Changed health, team, generation and trap clocks form a declared foreign block
beside unit intents. Native building/trap kernels own incoming damage and repair;
foreign callbacks cannot enter old combat or collision adapters. Props retain
animation and world bookkeeping until their phase moves.

`AnimatedProp.seize/carry` submits a qualified command with a prepared clip and
foreign node frame. Rust freezes motion, samples the soldier root, places the
body and retains the final frame for lazy limb sampling. Seizure does not advance
the shape stamp; a successful carry increments `tickCount` once. Missing clips or
joints leave placement unchanged. The held object, matrix and body vectors keep
stable JS identity. F2 supplies native rider placement, samples and visibility,
including hidden corpses of destroyed enclosed hulls.

## Registry, queries and combat

The native mixed registry retains insertion order. Strict ray ties select the
first member. Ordinary sight rays pass up to two excluded endpoints directly to Rust; custom
predicates and longer exclusion lists supply a foreign acceptance mask. Sphere
observations publish only bounds. A separate JS limb-read stamp lets hitShapes
fetch the canonical list later without sampling it again.
Unit shape caches remain lazy: publication, damage and death do not
refresh them. Tick, spawn, successful placement and each separation pair preserve
the original invalidation points. Retained skeletons support exposed and held
bodies. Separation includes lingering corpses and skips held or seated bodies.
`PathExecutor.contactSidestep` consumes its avoidance vector through a narrow
command without invalidating that cache.
Hull target facts share F2's physics/seat revision. Repeated sensor and history
reads reuse one fact row until a hull store or command invalidates it.

Combat uses prepared numeric F1 services and retained shot/muzzle buffers. Instant
and lag-compensated rounds cast canonical shapes or native history. History keeps
capture order, the 48-tick ring, gameplay-life rejection, teleport cutoff and old
stance topology. Saved shapes retain one full-precision half extent rather than
duplicating the live shape's publication-only halfSize vector. Moving projectile
sweeps use only moving targets; they cannot
replace the static/terrain terminal already retained by flight.

Damage and explosions complete immediately and depth first. Index iteration uses
the live registry in unit order, then the explosion journal, hulls, items, props
and recon. Nested deaths finish before the next index. Hull damage uses F2's
existing solver and clears crew leases before killing occupants. Enabled collision
bytes change immediately, so later rounds can pass through a newly disabled hull.
Melee retains its first ten candidates and reverse strike order. Arc acquisition
keeps stable along-distance ties, mines before recon and original RNG draws.

F1 projectile notices retain launch and terminal samples beside stable identity.
Impacts drain before retirement, preserving capture launch identity. Items use
canonical slabs, ordered target handles and native health/ammunition writes.
Power-up effects and pickup observers follow in source order after the return;
hero health does not pass through a 16-bit field. Dispenser lists preserve lazy
owner numbering, empty-list deletion, reactivation order and old-life attribution.

## Recon and randomness

Recon contexts, flight, weapons, camera/FOV and detached strikes are native.
Creation notices bind ordinary droid facades before later references decode.
Network slots retain terminal objects when a later launch reuses the slot.
Opaque connection keys keep their original seed and capture meaning; retained
numeric cookies name native contexts. Rebind-before-spawn follows the live owner
without rewriting immutable launch attribution. Salvo definitions refer to the
actual droid arsenal's prepared level objects.

Numeric bindings exchange shared Classic and named RNG state at phase and aimer
boundaries. Named streams remain lazy and preserve first-use insertion order.
Classic's post-board generator attachment is an explicit qualified lease command,
so a later mirror cannot restore the default mounted stream. No draw callback or
mutating setter export runs from the consuming phase.

## Instrumentation and construction

The shared phase counters record crossings and scalar words at their copy sites.
The native allocator records actual calls, reallocations and requested bytes
around the owning phase and capture, including serialization and callbacks. A
bounded trace identifies initial allocation sites without limiting total counts.
These native counters do not measure JS allocations. Synthetic fixtures exercise
the owning entry without game files; battle measurements are separate evidence.
The debug mirror detector compares ordinary values without property traps.

`f3-combat.js` loads immutable content and initializes combat records during
construction. UnitFactory calls adoption explicitly after completing each body,
and delegates acquire/release through the native owner. The bridge never wraps
factory constructors or replaces the WASM export table. Adoption binds qualified
command methods to the ordinary mirrors; getters read those mirrors directly.
The facade borrows the read-only `PlayerCombat.eye` camera helper. Page combat
retains camera aim, held-weapon presentation, charge glows, flashes and HUD reads.
It has no update, hit, blast or damage implementation. Page recon prediction
runs a private recon authority in `ClientPrediction`, using the shared native
collision world and water map. `RemoteDroids` retains meshes, view resources
and packet state; it contains no controller or weapon-update loop.

Pose construction requires the battle's native sampler. Drawn bones receive its
normalized results; gameplay reads native hidden rigs, including rider poses.
There is no JavaScript clip sampler or animation selection.

UnitFactory keeps inactive construction stock outside the mixed registry. Cold
preparation transfers the complete canonical row and its retained movement,
muzzle, pose and targeting buffers. Activation assigns the next mixed id at the
original acquisition point and emits a generated numeric birth notice before
any Unit references are decoded. A warm acquisition keeps the existing pooled
Unit and vectors. Future mixed-id capacity is reserved before player admission;
a missing prepared class is an explicit capacity failure.

Player admission resolves existing combat, spread and recon stream identities
without drawing or materializing lazy named streams. Native player realization
removes the connection's old class body before acquisition, preserves the local
viewpoint's Unit identity, rebinds recon, and spawns once. Weapon stream bindings
follow reset's fresh weapon state. Stream notices update retained callbacks
directly, so mirror refresh cannot restore construction's original generator.
All lifecycle services borrow the same generated Store and ownership sidecar;
supply, held-body, damage, combat and registry readers use those canonical rows.

A viewpoint's inactive class bodies stay private to its qualified Unit. Class
changes move the outgoing row and rig back to their resident cookie; another
connection cannot acquire them. Returning to that class retains its original
body vectors. The initial passive viewpoint registers its resident resources
outside ticking, before any class replacement can need them.

Publication reuses the fleet delta encoder. Each Unit retains the last consumed
encoding and the pending changed-word list; the reader acknowledges its version
in a generated row. Repeated command or aimer publications accumulate until that
acknowledgement. A value restored to its consumed baseline removes its pending
word, so an unchanged row sends nothing. Dirty-slot stamps coalesce touches in
first-touch order without scanning the already-touched list.

Combat shot, muzzle, melee and arc work storage is shared by the serial phase.
Cold body construction reserves publication capacity and immutable weapon flags.
Relations use an indexed numeric table; live sensor loops do no key formatting.
Invalid command fields, stale lives, absent native services and excessive damage
nesting are reported at their owning boundary rather than trapping the module.

The unit reader decodes retained native publication rows directly. Small motion
records update as one group; unchanged membership and visibility facts do not
reapply to public Units. Weapon scalar getters read only the requested generated
word. Full weapon observations refresh their existing mirror, while camera basis
vectors refresh in place only when observed. None of those reads calls a module
export. Construction and arsenal replacement still initialize complete mirrors.

Rewind frame capacity follows current registry members and cold construction
stock, including prepared hulls. Numeric admission capacity is separate: unused
future ids do not each reserve limb buffers in every history frame. Late cold
preparation enlarges this bound before any admitted body can appear in a tick.

Registered random streams share one native draw epoch. Aimers observe that word
before reading stream rows, so an idle boundary neither scans nor decodes the
whole registry of generators. A changed epoch retains native first-use order
when lazy named streams become visible to JavaScript. Foreign target inputs
also retain their own cold membership list; infantry is never polled for facts
already owned by the native store.

HitHistory.capture retains a generated, generation-qualified bounds sample after
its ordinary lazy shape observation. Unit.sphere can read that sample directly
when the public tick agrees. Native movement and rider touches invalidate it;
no publication computes a new shape merely to populate the mirror. Respawn and
body placement keep the original same-tick shape invalidation rules.

The completed control reader always consumes one-shot intent buttons. Native
MoveInput and latch mirrors follow their dirty groups, preserving the reserved
jet-direction vector across absent intervals. Page prediction uses its separate
input object and keeps its existing writes.
