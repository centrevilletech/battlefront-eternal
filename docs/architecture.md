# Architecture overview

`bfe-client` is the standalone native game binary: it opens a native desktop
window, owns its OpenGL ES 3.0 / OpenGL 4.5 context and `cpal` audio device,
runs the shell menus, server browser, and battle UI overlays in Rust, steps
Instant Action battles on a dedicated 60 Hz local world thread, and connects to
`bfeserver` instances over `BFE-UDP/1`. `bfeserver` (`rust/bfe-server`) is the
standalone native dedicated server binary, running the same simulation crate
without a renderer or audio device.

For building, testing and schema generation, see [development.md](development.md).

## Engine ownership

| Area | Implementation | Current responsibility |
| --- | --- | --- |
| Authoritative world | `rust/sim/rust/world/` | One ordered world tick over battle-local owners, including water, squads, cameras, AI, units, vehicles, recon, live objects, ordnance, items and battle rules. |
| Units and combat | `rust/sim/rust/units/`, `rust/sim/rust/combat/`, `rust/sim/rust/recon/` | Bodies, pooled lives, collision shapes, weapons, damage, hit history and recon droids. |
| Vehicles and battle | `rust/sim/rust/vehicles/`, `rust/sim/rust/battle/` | Hulls, flight, seats, riders, turret crews, spawning, objectives, roster and statistics. |
| AI | `rust/sim/rust/ai/` | Classic and Enhanced decisions, navigation, perception, aim and class jobs over canonical world state. |
| Prediction and publication | `rust/sim/rust/client/`, `rust/sim/rust/prediction/`, `rust/sim/rust/wire/` | Private own-player replay and reconciliation, authoritative snapshot capture, budgets, deltas and encoded publication. |
| Collision and movement | `rust/sim/rust/collision/`, `rust/sim/rust/movement/` | Map and dynamic-body queries, soldier movement and retained query scratch. |
| Presentation | `rust/bfe-presentation`, `rust/bfe-pose` | Displayed bodies, hulls, riders, props, cameras, items, projectiles, effects, pose sampling and received snapshot timelines. |
| Rendering | `rust/bfe-render` | Battle frame schedule, shader catalogs, geometry/material descriptors, textures, targets, cameras, lights, draws, post-processing, HUD and interface geometry. |
| Environment | `rust/bfe-environment` | Destruction, weather, atmosphere, water and shore state, foliage clocks and placed effects. |
| Audio | `rust/audio`, `rust/audio-control`, `rust/audio-decode` | Sample rendering and master processing, battle sound decisions and asset decoding. `cpal` delivers the rendered samples to the audio device. |
| Content and codecs | `rust/content`, `rust/bfe-content-native`, `rust/image-codec`, `rust/texture-codec`, `rust/basis-transcode` | Prepared content loading, image decoding and Basis texture transcoding. The Basis boundary includes vendored C/C++ dependencies listed in the third-party notices. |
| Native client and UI | `rust/bfe-client`, `rust/bfe-shell`, `rust/bfe-interface`, `rust/bfe-battle-ui`, `rust/bfe-online`, `rust/bfe-desktop` | Standalone native window/GL/audio host (`Startup` -> `Gate` -> `Shell` -> `InstantAction` / `Online`), shell menus and options, battle overlays (spawn, scoreboard, pause, chat, loading), server browser, 60 Hz local world thread, and desktop platform services. |
| Dedicated server | `rust/bfe-server`, `rust/bfe-transport` | Headless 60 Hz battle authority (`bfeserver`), encrypted `BFE-UDP/1` sessions, master registration, signaling and administration. |
| Shared arithmetic | `rust/jsrt`, `rust/three`, `rust/v8math` | Collection and number semantics, geometry math and cross-architecture deterministic simulation arithmetic. |

## Hosts and boundaries

`bfe-client` (`rust/bfe-client/rust/app/`) is the native desktop host.
It runs the shell (`bfe-shell`), battle UI overlays (`bfe-battle-ui`), online
server browser and identity (`bfe-online`), and platform services
(`bfe-desktop`) in a single process. In `InstantAction`, `bfe-client` spawns a
dedicated 60 Hz local world thread (`rust/bfe-client/rust/local/`) that communicates
with the render thread exclusively through a pre-allocated non-blocking command
ring and publication slot pool (`try_lock` only on the render thread), feeding
the same `Receiver` / `Sender` / `Replay` / `Scene` presentation pipeline used
by online battles. In `Online`, `bfe-client` exchanges `BFE-UDP/1` packets with
`bfeserver` directly. See [native-client.md](native-client.md).

`rust/sim/rust/world/owner.rs` groups the canonical store and phase owners for
one battle. Internal phase services borrow that `World`, and each battle's
native `Context` owns a boxed `World`. Generated block readers operate on
`BlockView` spans.

Boundary layouts come from declarative JSON schemas and `cargo xtask`
generators (`rust/sim/schema/*.json`, `rust/bfe-presentation/layout.json`,
`rust/bfe-environment/layout.json`, `rust/bfe-transport/layout.json`,
`rust/audio/layout.json`, `rust/audio-control/layout.json`,
`rust/audio-decode/layout.json`, and `rust/bfe-render/shaders/`).
Entity references carry generation-qualified identities. Changed rows and
ordered events cross at completed boundaries; consumers keep retained storage.

## Native runtime responsibilities

In `bfe-client`, window management, menus, spawn and scoreboard interaction,
keyboard and pointer events, content verification and extraction, `cpal` audio
routing, update checks, Steam/Discord rich presence, local 60 Hz Instant Action
authority, and online `BFE-UDP/1` sessions all run natively in Rust.

Rust owns the battle's frame schedule and GPU resource lifetime (`bfe-render`
and `bfe-presentation`).

For timing and data flow, see [runtime-architecture.md](runtime-architecture.md).
For server acceptance evidence and its limits, see
[host-qualification.md](host-qualification.md).
