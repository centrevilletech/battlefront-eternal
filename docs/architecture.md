# Architecture overview

Battlefront Eternal runs as an Electron desktop app (and in a development
browser) with a dedicated server on Node. Most game logic, presentation state,
rendering and audio processing is written in Rust under `rust/`, built to
WebAssembly by `tools/build-rust.mjs` and committed into `src/` (base64
modules such as `src/sim-wasm.js`, or wasm-bindgen packages such as
`src/rust-pkg/`), so running the game needs no Rust toolchain. The JavaScript
in `src/` is the host: it loads those modules, moves numeric blocks across
their boundaries and talks to the browser, Electron and the network.

Block layouts on every boundary are generated, not written by hand:
`tools/sim-schema.mjs` makes the simulation's store and record code from
`rust/sim/schema/`, and the other crates publish their layouts the same way
(for example `rust/bfe-environment/layout.json`). See
[development.md](development.md) for building and checking the Rust parts.

## Crates and what they own

| Area | Crate | Owns today |
| --- | --- | --- |
| Simulation | `rust/sim` (module `bfe_sim`, `src/sim-wasm.js`) | The 60 Hz authoritative world: collision queries, soldier movement, ground vehicles and flight, ordnance, items, combat and weapons, objectives, spawning and the battle rules, the entity store, client prediction and reconciliation (`client/`, `prediction/`), snapshot capture and encoding (`wire/`) and the battle AI (`ai/`, both Classic and Enhanced). Loaded once per thread by `src/sim/module.js`. |
| Simulation support | `rust/content`, `rust/jsrt`, `rust/three`, `rust/v8math` | Game content parsed as the simulation reads it, and JavaScript, three.js and V8 `Math` arithmetic reproduced bit for bit so a battle hashes the same on every host. |
| Presentation | `rust/bfe-presentation` (`src/presentation-pkg/`) | Persistent presentation state between ticks: cameras (soldier, vehicle, spawn, death and shake), unit, vehicle, prop, turret, item and ordnance presentation, lights, interface batches, small page policies such as the leaving-battlefield warning, and the page's network phases (`network/`): received snapshot bytes are decoded against their baselines and sampled in Rust, and outbound input records are built there. |
| Pose | `rust/bfe-pose` | The animation pose sampler and rigs (riders, hulls, droidekas, first person) shared by the simulation and presentation, and the bounded snapshot timeline that samples remote bodies and hulls between received snapshots. |
| Rendering | `rust/bfe-render` (`src/rust-pkg/`) | The battle renderer over WebGL 2 through glow: static world geometry and material descriptors, terrain, foliage, canopies, far scenery, water, shadows, transparent passes, effects, HUD and interface drawing, and the post chain (bloom and upscaling). `src/rust-render/` supplies resources and per-frame facts. |
| Environment | `rust/bfe-environment` (`src/environment-wasm.js`) | Destruction and weather presentation: pooled particles and lightning arcs, snow, storms, sky and atmosphere, and the world surfaces' per-frame state (water, shore, wakes and foliage clocks). |
| Audio | `rust/audio` (`src/audio-dsp-wasm.js`), `rust/audio-control`, `rust/audio-decode` (`src/audio-codec-wasm.js`) | `audio` renders every quantum (playback, streams, placement, reverb and room sends, master processing); `audio-control` owns sound state and decisions from the facts the battle publishes; `audio-decode` decodes assets on a loading worker. |
| Codecs | `rust/image-codec`, `rust/texture-codec`, `rust/basis-transcode` | PNG decoding and KTX2/Basis transcoding (`texture-codec` wraps `basis-transcode`) on the loading worker, so the renderer receives prepared pixel and block data. |
| Node host | `rust/bfe-node` | A Node-API addon for the dedicated server that currently supplies context lifecycle and a checked numeric block boundary. Battles on the server still step in the WebAssembly simulation; native world stepping is planned. See [node-host.md](node-host.md). |
| Native client | `rust/bfe-client`, `rust/bfe-native` | A windowed native host (winit, OpenGL ES, cpal audio, generated input records) that replays captured renderer scenes through `bfe-render`. It does not load or simulate a live battle yet; see `rust/bfe-client/README.md`. |
| Tools and checks | `rust/sim-replay`, `rust/v8math-check`, `rust/glbench` | Replays of recorded simulation captures against the Rust ports, checks of `v8math` against V8, and a WebGL call-cost benchmark. |

## What stays in JavaScript

The browser and Electron host remains JavaScript, because these are browser or
platform APIs rather than game logic:

- **DOM menus and front end**: the shell, menus, spawn screen, scoreboard,
  pause and settings screens (`src/shell/`, `src/menus/`, `src/spawn-screen/`,
  `src/scoreboard/`, `src/pause/`).
- **Input events**: keyboard and mouse listeners and pointer lock
  (`src/input.js`), which write generated input records for the simulation.
- **Loading**: `fetch` of game data and assets, and the loading workers that
  hand files to the Rust decoders.
- **Audio device**: the `AudioContext`, its AudioWorklets (`src/audio/worklets/`)
  and the stream and codec workers that feed the Rust audio modules.
- **Electron**: the main process, preload and IPC (`electron/`), game data
  discovery and preparation, LAN and network processes, updates and presence.
- **Networking glue and world assembly**: the network session, transports and
  the bindings that hand packet bytes to the Rust network phases
  (`src/runtime/page-network.js`, `src/runtime/` and `src/net/`), and `src/game.js`, which
  assembles the world and calls the Rust phases in order on the page, the
  world worker and the dedicated server
  ([runtime-architecture.md](runtime-architecture.md)).

Some gameplay and presentation code is still JavaScript while the port
continues (for example parts of the vehicle, weapon and world setup modules in
`src/`). Open porting work is tracked in GitHub issues.
