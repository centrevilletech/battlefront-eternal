# Presentation records

`rust/bfe-presentation` owns effect particles, displayed unit, hull and prop
poses, their clocks, world matrices, skin records, cameras, ordnance and items.
The kernels run without a browser or GL context. Browser bindings expose fixed
buffers and resource registration. A
module that fails to load, or a page built against another layout, is an
explicit error; these phases have no JS path. A full store never fails a frame.
Dedicated servers do not construct a displayed presentation owner.

The web module requires WebAssembly SIMD128 for its pose and matrix lanes.
Native builds use the same arithmetic and recorded replay results.

The effect phase consumes one ordered command batch. Body poses and first-person
attachments use two batches: body poses, the host camera phase, then first-person
matrices and attachments. Death and rider cameras consume the current body;
first-person matrices consume that frame's pre-shake camera before effects read
the gun, including on undrawn frames. Camera policy also runs in this crate.
The own camera precedes body sampling so its motion cue reaches the arms that
frame; death, recon and seat cameras consume the preceding body batch. Shared
retained scene ownership is still needed to combine these memory owners.

The ordnance phase draws the rounds a page shows: bolts, beams, rockets,
shells, missiles and grenades. The page writes one record per round to draw,
in draw order, and a marker where the replicated projectiles belong among
them; one call returns the instanced quads for each particle batch, the
blaster capsules, and an ordered list of what the host does with its own
objects (light requests, sniper trace touches, mesh and trail moves, a
round's stored muzzle offset). Replicated projectiles keep their samples,
flight past the last one, and smoothing in the kernel, by slot; the page keeps
only each one's mesh and trail. The kernel carries a bolt's first-draw and
flicker state, keyed by the bolt's slot and generation (or a page key), and
recycles it after 120 undrawn frames.

Enhanced sniper traces retain their flights, vapour, waiting head shots,
impact callbacks, sparks and draw buffers in `sniper.rs`. Collection follows the
round batch; due terminal points match in one batch before settlement; final
drawing advances every retained trace once. Numeric event ids carry resource
callbacks back to the page. Reserved effect seeds preserve the original random
order without a callback from the kernel. Bounded stores evict visual records
and release their pending handles.

Visual walker feet use `feet.rs`, sharing audio's native crossing detector.
Cold clip samples retain bind transforms, authored scales and f32 height stores.
One frame spans hulls and placed objects; it emits only planted-foot resources
at the displayed primitive's turn and the hull root's height.

Items a page online copies from the server's records have their own phase
(`items.rs`). One call a frame flies and places every copy between its
records (extrapolation, reconcile and adopt of `EntityItemPresentation`, the
flight, contact and placement of the world's items). The kernel calls nothing,
so each tick of flight asks the page for a ray against its collision and, for a
mine, a water crossing: the phase returns request rows, the page answers them
into a typed block and calls `resume`, and the rounds repeat until none are
left, up to a round limit per frame. The limit and a clock that stalls or runs
wild leave items where they are; a record whose id, definition or slot the
store cannot take is refused and counted, never thrown. Models are the page's:
it registers each definition's numbers once, makes and removes meshes from the
view serials the kernel publishes, and copies the placed position and
orientation onto them and the light request into the light pool.

## Layout and order

`rust/sim/schema/presentation-layout.json`, `timeline-layout.json`,
`ordnance-layout.json` and `items-layout.json` declare numeric records, command kinds and capacities.
Snapshot sampling, ordnance and items have separate phase files. `node tools/presentation-schema.mjs` generates both languages;
`--check` rejects stale output. Inputs and poses use f64, while final skin matrices
retain the renderer's f32 rounding. Resource registration resolves names, clip
aliases, bones and banks once. Unit display and simulation share `bfe-pose`'s
clip sampler and rigs.

Frame, content epoch, authority tick, input clock, playout tick and visual dt are
separate values. Each effect birth carries its generation and 32-bit local seed
(one draw of the page's `Math.random` at birth, or the caller's). Every random
choice for that effect's particles (how many a burst makes, each one's size,
lifetime, speed, spin and colour within its ranges) comes from that seed's own
stream, by the same formulas the JavaScript particles used: a frozen JavaScript
reference fed the same per-effect streams draws the same pixels. Only the dice
changed: builds before the Rust phase drew every particle from the page's one
shared stream, so their frames differ wherever effects are alive.
Commands precede advancement. Effects step in birth order, emitters in definition
order and particles in birth order. Children join the tail and step that frame.
Burst births precede expiry checks. Killing a parent kills its children; ordinary
expiry stops their emission. Death events follow the last particle death.
Map sources can queue a prewarm at birth; its temporary draws are replaced by the
following whole environment batch, while its births and clocks remain ordered.

Unit commands retain arrival order. Body interpolation, sampled firing-counter
edges, LOD clocks, held/gunner poses and rider placement precede world and skin
publication. The attachment phase advances no body clocks. Current first-person
pose state advances once, and the second phase places it with the camera record.
The own body is placed (world matrices, skin and node records) in its phase,
since the camera follows it. Other soldiers publish only their root place there;
like the scene update that skips hidden and off-view subtrees, the page places
the soldiers it will draw in one call before that update, and places any other
soldier on the first matrix update asked of it.
Foreign scene/interface parent matrices are records too. A paused class preview
uses a small instance of the same kernel, with its interface parent supplied
after the UI places it.

Ordnance stores reserve 4,096 records and network commands a frame, 8,192
host calls, 64 batch regions of 1,024 quads, 4,096 blaster capsules, 4,096
bolt views and 4,096 network projectile slots, above what a battle draws.
Past a store the kernel drops what it cannot hold, counts it in the header,
and the page reports the totals once; a full network command block is applied
early rather than lost, a slot past the table is skipped, and no frame fails.
`ordnance-replay` replays a capture (`beginCapture` on the page) and requires
every output bit to match.
## Vehicles, mounts and props

A world's hulls spawned at load are seeded natively: their pose import
(`veh_pull`) poses the spawn the way the hull's render does at a tick
(walk pose, lifted legs, its own gear, posed seats, barrels at rest) before
the first phase. The world keeps no JavaScript pose of its own.

Vehicle construction consumes model, class and clip records. Rust builds the
bind skeleton, muzzle markers, aimers, seat views, wheel axes and tow-leg
geometry. It also packs rigid and soft-skinned material batches. The host
instantiates Three resource shells from those results, including the native
skeleton inverses; it does not rebuild the numeric rig. Collision resources,
weapon objects and prediction facades remain host resources.

A craft drawn from its records keeps its gear clock in the hull phase:
held down through the lift, stowed over TakeoffTime once up, lowered over
the landing. The host says only which life its records are followed for.

The hull phase places all queued roots and writes one block of CastRay
records: each walker's feet and the ground under each lifting craft whose
gear follows its records. The host casts the whole block in one call into
the simulation module's collision, with each hull's own body left out and
every body's enabled flag sent once; the phase then reads the hits and
poses every rig. The block is sized at rig registration for every entry,
so a frame never grows it. Each call covers the whole batch. The own
predicted hull uses a second batch because tethers read the authoritative
placement first. Cold registration reserves the two slots and updates the
host's cached capacity; frame entries never query WASM individually.

The unit phase borrows native hull and prop source stores directly. The page
passes resource ids and a source revision; it transports no rider or held
matrices, sampled coordinates or duplicate bind hierarchy. Four native source
publications span ordinary and predicted hull batches. A body retains its
lease-birth sample independently of later source-history turnover.
Frames with no seated or held body skip source resolution entirely.
It hides enclosed riders, retains hidden crew after a hull's destruction
until their next generation, and places exposed riders after animation. A
creature's posed pelvis meets its saddle; chest sway keeps its native clock.
Hull rider clip selection retains the lease-time sample, taken before the
next hull phase advances its clock. A birth output also supplies the legacy
body coordinates still read by prediction and hit shapes. These touched
source fields publish once per lease; displayed matrices remain native.
World rider collision samples the unit's retained BodyPose through the native
vehicle owner. Held shapes use the same unit sampler and native prop frame;
Unit.updateSpheres and the page riderPoints solver have no JS implementation.
Page registry observations submit only stale bodies through one retained actor-id
prefix. Direct bounds and limb readers request that same coarse batch; unchanged
samples copy no inputs or outputs and make no native call. Body display neither
prepares nor observes collision shapes. Both owners share the posture primitives
and broadphase arithmetic in bfe-pose, preserving each tick's lazy observation.
Bounds observations copy only the centre and radius. Limb readbacks wait for
a narrowphase reader and share the retained native sample.
The prop display owner registers a held body from the native unit skeleton
and snapshots its current local pose once. Unkeyed grab joints retain that
seizure pose while bind lengths retain their immutable class values. No
skeleton or displayed bone array is rebuilt through JS.
A stationary turret's own gunner consumes its current preview each frame.
Replica gunners follow the current drum, refreshing their clip sample and
source interpolation each displayed frame, as placeReplica did. The body turn
still waits for its normal animation LOD sample. The own body follows its
seat while its prediction position keeps feeding lighting and view helpers.
Shared field-forwarding
prototypes preserve live source reads without making soldiers into prototypes,
which would box each refreshed aim value.

Stationary turret drum and gun matrices join the prop phase. Its placed roots
retain their authored matrices and scene parents. Props publish the eased
Float32 stand pose, including unweighted bones, rather than the sampler's
unblended target. Mesh bind inverses follow their own attached-mode frames.
A creature that seizes takes the world's latest state in the prop phase
(the first stands at once, later ones ease in) and hangs whom it holds
from its posed grab node: his grab clip at his time into the hold, his
root bone on the node. The host resolves who he is on its page, registers
his class's skeleton once and copies the frame onto him for his body,
camera and hit shapes.
Skeleton record order can place a parent after its child (the rancor and Jabba
use this order). Scene registration traverses parents first and maps every
bone explicitly; numeric bone ordinals never stand in for scene node handles.
Only changed matrix rows are copied to resource shells. Bone TRS writeback
and Three's subsequent rig walks are absent from these page paths.

The active snapshot timeline also samples in one native batch. A bounded
ring retains 64 snapshots and 1,024 identities; sparse ids, partial packets,
generations and discontinuities remain explicit. Hulls use normalized slerp;
body yaw takes the shortest arc. Missing brackets coast for six ticks and
then hold. The receive owner and the body/hull phases share the same native memory.
One frame call samples the ring; native resource links then supply historical
inputs directly to those phases. No sampled pose crosses through a JS object. Native bindings retire a sampled
life/class mismatch at that same boundary. Cold admission reads one generated
liveness flag to admit the same resource again on a later semantic packet.
Own predicted bodies and hulls bypass remote drawing without changing history.
A full ring reuses its oldest snapshot, and departed identities release their
slots. Closing a battle returns its native ring for reuse; stale callbacks
cannot read or overwrite the next battle's samples. Additional simultaneous identities or views are
refused and reported once rather than growing or failing a display frame.

SnapshotClient's multipart assembly and immutable ACK baselines also live in
that owner. A packet copies bytes into one reserved input span and returns only
changed gameplay records, seat changes and opaque owned payloads. Baseline
entries remain native. Cold diagnostic captures can inspect them for replay;
the live page never reconstructs them. Outbound cumulative look, held backups,
ACK accounting and binary input encoding use the same required module. Server
snapshot publication remains in its authoritative owner.

Receive storage bounds 512 protocol slots, 128 acknowledged baselines and
32 unfinished snapshots. Field payloads use reference-counted native handles;
complete versions share unchanged bytes. A paused receive queue discards its
oldest replaceable quarter when full, leaving incomplete snapshots unacknowledged
so the host repairs them. Ring and packet stores reserve once and return to a
cold pool at battle disposal. Neither packet assembly nor sampling allocates
native storage after setup.

## Storage and replay

Effect storage reserves 4,096 effects, 16,384 emitters, 65,536 particles/draws and
131,072 events, far above what any map's content reaches in battle (at most 54
effects, 137 emitters, 765 particles and 727 events a frame across the 17 maps).
When a store is full the kernel refuses the birth whole, as the old page's
pools grew instead: an effect without a slot, enough emitters or event room is
not born, and the page frees its handle from a finished row; a refused page
start returns a handle that is already finished and ignores every call. A
birth always reserves its death event, so births and deaths pair. Stale,
unknown or malformed commands are skipped; a full command block leaves the
remaining effects for the next frame. The header carries running totals of
everything refused or skipped, reported once on the page. Unit storage reserves 1,024 actors, 131,072 nodes, 16,384 commands
and 256 weapons. Kernels reuse their storage during frames. The host sends changed
commands and publishes only changed output rows; it does not regenerate native
hierarchies or skin matrices. A placed node's Three position, rotation, scale and
matrices are plain fields written from its record, so every vector and matrix
keeps one shape for Three's own code; authored values stand until the node's
first placement.
Host handles and renderer resources may allocate
at birth or first use, outside the kernel's allocation guarantee.

`presentRecord=1` records effects from their first frame into a bounded prefix.
Late recording is rejected because initial births are required. `endCapture()`
exports descriptors and numeric inputs/outputs outside the frame path. The native
`presentation-replay` and `unit-kernel-replay` tools validate recorded output bytes.
Extracted descriptors and captures stay outside the repository. CI uses synthetic
clip/particle fixtures and recorded output goldens, with native/WASM batch checks
and allocation counters.

`profileRuntime=1` exposes inclusive `effectsUpdate`, `timelinePresentation`,
`unitPresentation`, `animation`, `camera`, `hudBuild`, `hudDraw` and
`outputPublication` spans. Unit presentation includes own, remote, vehicle/rider
and first-person work. Animation measures the native pose call, with the own body's
matrices; other drawn bodies are placed within the render span; HUD draw includes submission. These
spans cannot be added to their enclosing phases. Performance comparisons use
immutable reference and candidate builds with matched battle inputs.

Cold soldier construction and soldier/droideka spawn resets use the native unit implementation
beside its collision store. Generated mirrors retain the resource and correction
fields that page consumers still read. The viewpoint's displayed gun basis and
eye publish with its native unit row. Remote bases wait for arc effects. Held
bolts name their source unit; the effect phase collects these and charging guns
for one native request prefix. A new bolt's shooter search requests every
candidate. Revisions avoid unchanged readbacks in either case.
predicted shots resolve all firing poses in one aim batch and one collision
resume. Preview publishes the physical gun basis separately from pending look,
so HUD targeting retains the tick pose. Immediate pointer preview submits one
ordered block for all pending samples, preserving per-sample pitch limits and
posture turns without advancing movement or mutating the corrected body. Empty
look frames make no native call; ordinary look computes angles without shot
geometry. Eye-view commands also need only body angles. Third-person command
convergence and shot publication retain their separate geometry phase.

## Cameras

`cameras::Phase` replaces SoldierCamera, FirstPersonMotion, CameraShake,
DeathShot, the world DeathCamera, VehicleCamera, TurretCamera and
updateFlightCamera. The page writes one generated CameraFrame and an ordered
CameraCommand prefix, calls `camera_frame` once, then publishes CameraOutput.
The exclusive priority is death, piloted recon, vehicle, turret, own soldier.
Shake follows selection; a spawn shot overrides only the final position and
turn. Both the pre-shake and final matrices, inverses and lens are native outputs.
Three transform callbacks and camera update hooks do not recompute them.

The camera kernel is linked into the raw simulation module to borrow its native
collision store. Corner rays, exclusions and death booms stay inside that call.
Two cold owners isolate the page clock from authority and private recon replay.
Cold projection/death-start and spawn-selection commands do not advance a
rendered frame. The spawn screen retains its HUD paint clock and cut notice;
its numeric shot choice, follow turn, pan and push run in the kernel. Revisioned
selection preserves a shot already submitted before a later UI event.

Generated CameraLimits bound two owners, 256 definitions, 1,024 commands, 1,024
spawn shots, seven motions with 64 segments, and 64 excluded collision bodies.
Collision flags use stable body slots in the 65,536-body store. A full shake
store admits four births in order; a full command queue drops its tail. A clear
cancels pending births even after a paused burst. Invalid ids use an empty
section or are ignored, and overlong motion steps have a fixed iteration bound.
None of these conditions throws or grows a frame buffer. Synthetic phase tests
include oversized paused bursts, every priority, frozen views and 1,200 frames
under a native counting allocator.

The WASM host supplies only standard scalar Math primitives and the cosmetic
random stream. Electron and the server's V8 can round elementary functions
differently; binding the current host's primitives preserves frozen camera bits.
Native callers select the same scalar trait, with V8 arithmetic supplied here.
Two- and three-argument norms keep their original argument count: appending
a zero can change the last bit of a distant death-camera radius.
There is no camera or scene callback. Scene handles, killer identity lookup,
body/head facts, visibility publication, DOM, pointer lock and raw input remain
host responsibilities. Frozen synthetic records cover motion, lenses, shake,
both death styles, a style transition and a distant death handoff without
requiring game files.


## World surfaces

`bfe-environment` advances water, shore, ocean lightning, foliage clocks,
weather, snow, atmosphere, sky flyers and placed-source state through one
`surface_frame` call. The page writes fixed numeric facts, then publishes
borrowed outputs at the existing resource boundaries. Foliage placement and
wind deformation already belong to the Rust renderer; only its native clock
is published here. Dedicated-server water physics remains a separate clock.

`Water.update`, `updateWaders`, `updateRings`, `WaterShore.update` and its
sampling helpers, `EnvironmentWakes.update` and `StormSky.strike` have no
JavaScript state implementation. The page retains geometry/material/texture
shells, particle-resource birth and kill application, collision queries and
mirror/probe/cloud draw hooks. This state phase does not schedule renderer
passes. Separate native owners still advance destruction and particles.

Surface births use a 4,096-row circular command block. A paused overflow drops
its oldest commands and reports the count; the water retains its original
newest 100 rings. Contacts use stable ids in 1,024 rows. Shore keeps at most
169 tiles, 2,048 unique points per tile, 16,384 collision triangles per sampled
tile and 384 visible patches (192 at High). Excess collision/point rows are
counted and skipped. Each frame samples at most two tiles, in the original
stable order. Wake storage retains 1,024 actor identities, 256 actor facts and
eight trail points, replacing the least recently seen identity at capacity.
Every frame reuses its native storage; resource views retain the shared heap.

The generated environment layout declares these records and limits.
`tools/surface-replay.mjs` records synthetic phase input, collision replies and
output bits; native `surface-replay` validates every result and draw attribute.
Synthetic tests cover long pauses, oversized births, actor identity churn,
cache movement and allocation-free native advancement. Electron supplies the
standard scalar sine, cosine, atan2 and two-argument norm used by the replaced
pixel-visible water and lightning math, following the camera import pattern.

## Static world resources

Opaque near/far world, terrain, foliage cards and canopy draw resources are
compiled and retained by `bfe-render`. The loading host supplies authored rows
and image assets once. Native source records own vertex/index bytes, layouts,
cloth subdivision, original-tree assignments, bounds and terrain levels. Colour
and static depth draws share those source records; scene nodes carry only
numeric resource ids and visibility. Cold lamp extraction reads authored rows
under its lighting owner, without constructing geometry or material shells.

Native materials select the closed program catalog and retain raster state,
uniform values and sampler ids. Static sampler ownership is pinned separately
from loader image ownership: late source uploads, sampler clones, release/reuse
and context restoration preserve the logical identity. Settings use typed
commands; static frame draws do not scan Three UUIDs, versions or properties.
`static-layout.json` generates both sides of the resource boundary.

The CPU source bank survives context loss. The existing recovery journal
replays GPU recipes against stable source ids, including ordered variant and
depth replacements. Shadow traversal supplies native descriptor ids and one
projection/view/time block per cascade; it preserves the existing pass order.
Depth admission retains at most 4,094 source references plus two merged sides;
excess sources clip in admission order with a counter. Renderer diagnostics
expose lifetime `shadowClipped.draws` (pass submissions) and
`shadowClipped.sources` (static admissions); the stats line shows their total
when nonzero. Cascade range selection
uses retained Morton cells and allocates no memory.

Standalone native fixtures freeze the replaced JavaScript constructors' vertex,
index, bounds, terrain/LOD, material and shadow-range bytes without content.
Blended world/sky, actors/rigs, dynamic water/effects and interface resources
retain their separate consumers until their resource families move.
