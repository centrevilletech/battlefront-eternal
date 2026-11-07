# Presentation records

`rust/bfe-presentation` owns effect particles and displayed unit poses, clocks,
world matrices and skin records. Both kernels run without a browser or GL
context. Browser bindings expose fixed buffers and resource registration. A
module that fails to load, or a page built against another layout, is an
explicit error; these phases have no JS path. A full store never fails a frame.
Dedicated servers do not construct a displayed presentation owner.

The web module requires WebAssembly SIMD128 for its pose and matrix lanes.
Native builds use the same arithmetic and recorded replay results.

The effect phase consumes one ordered command batch. Body poses and first-person
attachments use two batches: body poses, the host camera phase, then first-person
matrices and attachments. Death and rider cameras consume the current body;
first-person matrices consume that frame's pre-shake camera before effects read
the gun, including on undrawn frames. Neither kernel calls the host. Moving the
camera owner into Rust will allow these two phases to join.

## Layout and order

`rust/sim/schema/presentation-layout.json` declares numeric records, command kinds
and capacities. `node tools/presentation-schema.mjs` generates both languages;
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
