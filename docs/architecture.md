# Architecture overview

The Electron app and development browser load Rust engine modules as
WebAssembly. The dedicated Node server uses the same simulation module without
a renderer or audio device. A separate native client runs the renderer and
presentation crates in its own window and can join online battles.

The committed modules let ordinary application builds run without a Rust
compiler. Changing Rust code requires the pinned toolchain and regeneration;
see [development.md](development.md).

## Engine ownership

| Area | Implementation | Current responsibility |
| --- | --- | --- |
| Authoritative world | `rust/sim/src/world/` | One ordered world tick over battle-local owners, including water, squads, cameras, AI, units, vehicles, recon, live objects, ordnance, items and battle rules. |
| Units and combat | `rust/sim/src/units/`, `rust/sim/src/combat/`, `rust/sim/src/recon/` | Bodies, pooled lives, collision shapes, weapons, damage, hit history and recon droids. |
| Vehicles and battle | `rust/sim/src/vehicles/`, `rust/sim/src/battle/` | Hulls, flight, seats, riders, turret crews, spawning, objectives, roster and statistics. |
| AI | `rust/sim/src/ai/` | Classic and Enhanced decisions, navigation, perception, aim and class jobs over canonical world state. |
| Prediction and publication | `rust/sim/src/client/`, `rust/sim/src/prediction/`, `rust/sim/src/wire/` | Private own-player replay and reconciliation, authoritative snapshot capture, budgets, deltas and encoded publication. |
| Collision and movement | `rust/sim/src/collision/`, `rust/sim/src/movement/` | Map and dynamic-body queries, soldier movement and retained query scratch. |
| Presentation | `rust/bfe-presentation`, `rust/bfe-pose` | Displayed bodies, hulls, riders, props, cameras, items, projectiles, effects, pose sampling and received snapshot timelines. |
| Rendering | `rust/bfe-render` | Battle frame schedule, shader catalogs, geometry/material descriptors, textures, targets, cameras, lights, draws, post-processing, HUD and interface geometry. |
| Environment | `rust/bfe-environment` | Destruction, weather, atmosphere, water and shore state, foliage clocks and placed effects. |
| Audio | `rust/audio`, `rust/audio-control`, `rust/audio-decode` | Sample rendering and master processing, battle sound decisions and asset decoding. Browser or native device hosts deliver the samples. |
| Content and codecs | `rust/content`, `rust/bfe-content-native`, `rust/image-codec`, `rust/texture-codec`, `rust/basis-transcode` | Prepared content loading, image decoding and Basis texture transcoding. The Basis boundary includes vendored C/C++ dependencies listed in the third-party notices. |
| Shared arithmetic | `rust/jsrt`, `rust/three`, `rust/v8math` | Collection and number semantics, geometry math and simulation arithmetic matching the reference runtime. |

## Hosts and boundaries

`src/game.js` admits content and resources, prepares external facts, calls the
world operation and delivers its completed notices. It does not dispatch each
simulation phase. `src/runtime/world-tick.js` uses the world's backend bridge;
WASM memory bindings live in `src/sim/wasm/`. Generated block readers operate
on `BlockView` spans rather than assuming every provider has linear memory.

`rust/sim/src/world/owner.rs` groups the canonical store and phase owners for
one battle. Internal phase services borrow that World. The browser uses a WASM
instance for each battle; the native Context owns a boxed World. Shared phases
compile for native hosts, but the production bundle still selects WASM. Native
store arenas and lifecycle support do not yet expose a complete production
world/publication backend; see [node-host.md](node-host.md).

Boundary layouts come from schemas and generators, including
`rust/sim/schema/`, `tools/sim-schema.mjs` and the renderer's layout files.
Entity references carry generation-qualified identities. Changed rows and
ordered events cross at completed boundaries; consumers keep retained storage.
Native records do not transport pointers as JavaScript numbers.

## Browser and Electron responsibilities

JavaScript still hosts DOM menus, spawn and scoreboard interaction, keyboard
and pointer events, fetch and loading workers, browser audio devices,
Electron IPC, network sessions, updates and presence. It also admits authored
resources and publishes remaining scene and transform facts used by rendering
and gameplay queries. Rust ownership of a draw or pose does not mean the DOM,
file system or every retained Three.js scene anchor has disappeared.

Rust owns the battle's frame schedule and GPU resource lifetime. Three.js does
not submit battle or interface draws. Retained scene objects and shared image
shells still serve host consumers; some cold utility program registration also
uses the browser compiler. Failures report explicitly instead of selecting a
JavaScript simulation or another battle renderer.

The native client's online path receives authoritative snapshots and runs its
own Rust transport, presentation and rendering. It does not run a local
multiplayer authority. Map inspection and recorded-session replay are separate
modes; see [native-client.md](native-client.md).

For timing and data flow, see [runtime-architecture.md](runtime-architecture.md).
For server acceptance evidence and its limits, see
[host-qualification.md](host-qualification.md).
