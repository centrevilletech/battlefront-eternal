# Architecture overview

`bfe-client` is the standalone native game binary: it opens a native desktop
window, owns its OpenGL ES 3.0 / OpenGL 4.5 context and `cpal` audio device,
runs the shell menus, server browser, and battle UI overlays in Rust, steps
Instant Action battles on a dedicated 60 Hz local world thread, and connects to
`bfeserver` instances over `BFE-UDP/1`. The Electron app and development browser
remain in the tree during the transition and load the shared Rust engine modules
as WebAssembly; the dedicated Node/native server uses the same simulation module
without a renderer or audio device.

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
| Native client and UI | `rust/bfe-client`, `rust/bfe-shell`, `rust/bfe-interface`, `rust/bfe-battle-ui`, `rust/bfe-online`, `rust/bfe-desktop` | Standalone native window/GL/audio host (`Startup` -> `Gate` -> `Shell` -> `InstantAction` / `Online`), shell menus and options, battle overlays (spawn, scoreboard, pause, chat, loading), server browser, 60 Hz local world thread, and desktop platform services. |
| Shared arithmetic | `rust/jsrt`, `rust/three`, `rust/v8math` | Collection and number semantics, geometry math and simulation arithmetic matching the reference runtime. |

## Hosts and boundaries

`bfe-client` (`rust/bfe-client/src/app/`) is the primary native desktop host.
It runs the shell (`bfe-shell`), battle UI overlays (`bfe-battle-ui`), online
server browser and identity (`bfe-online`), and platform services
(`bfe-desktop`) in a single process. In `InstantAction`, `bfe-client` spawns a
dedicated 60 Hz local world thread (`rust/bfe-client/src/local/`) that communicates
with the render thread exclusively through a pre-allocated non-blocking command
ring and publication slot pool (`try_lock` only on the render thread), feeding
the same `Receiver` / `Sender` / `Replay` / `Scene` presentation pipeline used
by online battles. In `Online`, `bfe-client` exchanges `BFE-UDP/1` packets with
`bfeserver` directly. See [native-client.md](native-client.md).

In the legacy browser/Electron host, `src/game.js` admits content and resources,
prepares external facts, calls the world operation and delivers its completed
notices. `src/runtime/world-tick.js` uses the world's backend bridge; WASM
memory bindings live in `src/sim/wasm/`. Generated block readers operate on
`BlockView` spans rather than assuming every provider has linear memory.

`rust/sim/src/world/owner.rs` groups the canonical store and phase owners for
one battle. Internal phase services borrow that World. The browser uses a WASM
instance for each battle; the native `Context` owns a boxed `World`.

Boundary layouts come from schemas and generators, including
`rust/sim/schema/`, `tools/sim-schema.mjs` and the renderer's layout files.
Entity references carry generation-qualified identities. Changed rows and
ordered events cross at completed boundaries; consumers keep retained storage.
Native records do not transport pointers as JavaScript numbers.

## Native vs. legacy browser responsibilities

In `bfe-client`, window management, menus, spawn and scoreboard interaction,
keyboard and pointer events, content verification and extraction, `cpal` audio
routing, update checks, Steam/Discord rich presence, local 60 Hz Instant Action
authority, and online `BFE-UDP/1` sessions all run natively in Rust without a
DOM, Node, or Electron dependency.

Rust owns the battle's frame schedule and GPU resource lifetime (`bfe-render`
and `bfe-presentation`). Three.js does not submit battle or interface draws.
While `src/` and `electron/` remain in the repository during the transition,
they are no longer required to launch or play the native client.

For timing and data flow, see [runtime-architecture.md](runtime-architecture.md).
For server acceptance evidence and its limits, see
[host-qualification.md](host-qualification.md).
