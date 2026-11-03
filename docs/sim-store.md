# The simulation's module

The world's simulation is moving from JavaScript to Rust, one group of
subsystems at a time, without changing a bit of any tick: the world worker,
the page's replay of its own soldier and the dedicated server must keep
agreeing with each other and with today's battles. This note describes the
module that work builds on.

## One module, one instance a battle

`rust/sim` is one crate and one WebAssembly module, `bfe_sim`. It holds:

- `collision`: the collision world's queries (casts, sphere gathers, overlaps,
  the ground, `terrainNear`, a hull's wall probes), which `src/ray-kernel.js`
  calls ([docs/performance.md](performance.md), "The collision's rays in
  WebAssembly");
- `movement`: the soldiers' movement (`Soldier.tick`), which
  `src/movement-kernel.js` calls;
- `pose`: the pose sampler of `src/anim.js`, which `src/anim-kernel.js` calls.

These were the crates `rust/rays` and `rust/anim`; they are modules of one
crate now, so a battle holds one instance with one memory instead of one per
kernel, and the simulation's later parts can call them without crossing into
JavaScript. Their exports keep their names, so the bridges did not change.

`tools/build-rust.mjs sim` builds it and writes `src/sim-wasm.js`, the
module's bytes as base64 (141 KB of WebAssembly, 199 KB of text, 73 KB
compressed). It is committed, like the renderer's package, so building the
game needs no Rust, and CI rebuilds it and fails on any byte that differs.
`src/sim/module.js` compiles it once a thread (`simModule()`) and makes an
instance for each battle (`loadSim()`): the world worker, a page and plain
Node (the dedicated server, `tools/world-hash.mjs`) load it the same way,
with no `.wasm` file to serve. `createGame` makes one instance when the
battle asks for one of its kernels and hands it to each
(`loadRayKernel(instance)`, `loadAnimKernel(instance)`); asked for none, a
battle loads nothing.

## Memory

A battle's instance starts with the module's statics and stack, and its
memory grows as the kernels copy the map's trees and the clips in (3 to 12
MB a map). Growing it detaches every view JavaScript holds of it, so every
bridge makes its views again after any call that can grow it (each checks
the view's `byteLength`, which a detached view reports as 0). One instance
shared by the kernels changes nothing there: a copy one kernel makes detaches
the other's views too, and each makes them again where it next reads them.
