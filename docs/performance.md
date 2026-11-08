# World and frame performance

Rust owns the authoritative world tick, AI, movement, combat, vehicles and
snapshot publication. Rust also owns prediction, presentation, sound processing
and the battle renderer. The browser and Electron hosts still pay for input,
loading, scene facts, menus, devices, IPC and transport. Moving a subsystem to
Rust does not establish a frame-rate improvement on every machine.

The production engine has one implementation of each ported phase. Compare a
candidate with a frozen reference build; there is no JavaScript simulation or
battle-renderer switch for performance comparisons.

## Measurements and their scope

The following measurements are retained from the project's earlier collision
optimization workload. Each sample ran 1,800 uphill movement ticks and 1,800
aiming rays over prepared map geometry, with the same starting state and live
bodies. Times are CPU milliseconds for the whole sample, not displayed frames.
They describe that change and workload, not the improvement of this release.

| Workload | Before median | After median |
| --- | ---: | ---: |
| Theed, 23 enabled live bodies | 283.50 | 135.22 |
| Bespin, 14 enabled live bodies | 513.48 | 222.90 |

Both runs produced identical final positions. Shared-host variance affected
timings; these numbers do not establish an FPS increase or a minimum hardware
requirement. [render-performance.md](render-performance.md) retains separate
historical renderer measurements, including their software-GPU limitations.

The target remains playable Performance settings on weaker PCs, with a fixed
60 Hz authority and identical battle rules across graphics settings. A 30 FPS
render target is not a guarantee for every CPU, GPU, driver, resolution or map.
Measure the release build and workload on the actual target device.

## Current ownership and costs

| Work | Owner | What to observe |
| --- | --- | --- |
| World phases | `rust/sim/src/world/step.rs` | Completed ticks, step cost, catch-up debt, allocations and ordered output. |
| AI | `rust/sim/src/ai/owned/` | Decision/LOS budgets, native queries, held intent publication and allocations. |
| Collision and movement | `rust/sim/src/collision/`, `rust/sim/src/movement/` | Query visits, retained scratch, dynamic-body state and exact final positions. |
| Hulls, riders and weapons | `rust/sim/src/vehicles/`, `rust/sim/src/units/` | Native substeps, rig sampling, touched rows and callback order. |
| Snapshots | `rust/sim/src/wire/` | Publication bytes, recipients, budgets, queues and retained baselines. |
| Prediction and displayed poses | `rust/sim/src/client/`, `rust/bfe-presentation` | Pending replay ticks, correction work, sampled poses and observed readbacks. |
| Drawing | `rust/bfe-render` | Submitted work, GPU completion, shadows, uploads, target size and frame pacing. |
| Sound | `rust/audio-control`, `rust/audio` | Active voices, decode/stream credits, device quanta and master-chain cost. |

World stores and phase scratch retain their working storage. Changed rows are
published sparsely; untouched fleets, weapons and bodies are not copied just
because a tick elapsed. Admission and resource construction are separate from
the warmed phase. Queue or visual-store saturation must follow its declared
refusal or degradation policy rather than stop a frame.

Texture source storage is shared by compatible samplers and released when its
last native alias is gone. Encoded assets and restore recipes have separate
lifetimes from visible draw instances. Memory measurements must distinguish
retained content, reserved capacity, used records and resources after close.

## Reproduce and check

`tools/benchmark-world.mjs` exercises collision and movement against prepared
Theed and Bespin content in the checkout's generated map directory:

```sh
node tools/benchmark-world.mjs
```

`PERF_ROUNDS` and `PERF_CYCLES` set its sample counts. An optional
`PERF_REFERENCE_COLLISION` points to an external frozen module exporting
`CollisionWorld`; it does not select an alternate implementation in the game.
Loading and renderer work are excluded from that workload. Its historical
JavaScript foliage sweep was removed; foliage is measured in Rust drawn frames.

For a complete battle, use a minified desktop build with the same content,
seed, population, input trace, graphics settings and camera in both versions.
Alternate candidate and reference order, retain warmup rules, and record device
and driver details with the result. Avoid background work that changes CPU or
GPU contention during one role.

`tools/desktop-world-advance.mjs` checks a real running desktop world's clocks,
player life and errors. `tools/world-hash-matrix.mjs` checks state, deep digest
and published bytes against frozen reference hashes. A stopped simulation must
not be credited as a cheaper page frame. Native and browser replay checks cover
shared arithmetic; pixel comparisons cover displayed output separately.

Correctness captures and load tests are not performance qualification. In
particular, software rendering cannot predict a physical GPU, and a fast GPU
test machine cannot establish CPU performance on an older laptop.

## Report separate budgets

Record page CPU, worker/server CPU, completed world ticks, GPU completion,
frame-time percentiles and input-to-present latency separately. Submission time
alone omits queued GPU work. A reduced back buffer can help a GPU bottleneck
without reducing simulation cost. A frame cap can reduce presentation work
without changing gameplay dt.

Measure loading, ordinary play, spawning and newly encountered effects, long
battles, pause/resume, focus loss and reconnect. Keep synthetic fixture results,
full battle observations and hardware acceptance evidence distinct. Server
memory, concurrency and termination have their own acceptance checks in
[host-qualification.md](host-qualification.md).
