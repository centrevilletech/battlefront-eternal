// The simulation's WebAssembly module, bfe_sim (rust/sim): the collision's
// queries (src/ray-kernel.js), the soldiers' movement
// (src/movement-kernel.js), the pose sampler (src/anim-kernel.js) and the
// entity store the simulation's port builds on (docs/sim-store.md), in one
// module and one memory.
//
// The module is compiled once a thread, from the base64 in src/sim-wasm.js
// (so the world worker, a page and plain Node all load it as a JavaScript
// import, with no asset to serve), and each battle gets its own instance,
// whose memory, and every copy and entity in it, go with the battle (a
// dedicated server loads one battle after another).
import { SIM_WASM } from "../sim-wasm.js";

let compiling = null;

// The compiled module (a promise of it), made the first time it is asked for.
export function simModule() {
  return compiling ||= (async () => {
    const text = atob(SIM_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return WebAssembly.compile(bytes);
  })();
}

// A new instance for a battle. `imports`: what the module imports from
// JavaScript (none yet).
export async function loadSim(imports = {}) {
  return WebAssembly.instantiate(await simModule(), imports);
}
