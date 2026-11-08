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
import { IMPORT_SYMBOLS } from "./gen/imports.js";

let compiling = null;

// The compiled module (a promise of it), made the first time it is asked for.
export function simModule() {
  return compiling ||= (async () => {
    // A native decoder where there is one (the copy loop below costs about
    // three times what atob does).
    if (Uint8Array.fromBase64) return WebAssembly.compile(Uint8Array.fromBase64(SIM_WASM));
    const text = atob(SIM_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return WebAssembly.compile(bytes);
  })();
}

// A new instance for a battle. What the module imports from JavaScript
// (a schema's imports) is served by the store's bridge once one is
// attached (src/sim/runtime.js SimBridge): until then an import throws.
export function simImports(holder) {
  const bfe = {};
  for (const imp of IMPORT_SYMBOLS) {
    if (!imp) continue;
    bfe[imp.symbol] = () => {
      if (!holder.bridge) throw Error(`the simulation's module asked JavaScript for ${imp.name} with no bridge attached`);
      holder.bridge.serve(imp.code);
    };
  }
  const cameraMath = {
    camera_sin: Math.sin, camera_cos: Math.cos, camera_tan: Math.tan,
    camera_atan: Math.atan, camera_atan2: Math.atan2, camera_asin: Math.asin,
    camera_acos: Math.acos, camera_exp: Math.exp, camera_log: Math.log,
    camera_pow: Math.pow, camera_hypot2: Math.hypot, camera_hypot: Math.hypot, camera_random: () => holder.cameraRandom(),
  };
  holder.cameraRandom = Math.random;
  const resource = name => (a,b,c,d,e,f) => holder.ordnanceResources[name](a,b,c,d,e,f);
  const ordnance_resources = {};
  for (const name of ['resource_kind','reference','number','boolean','index','numeric_index','dynamic','read_vector','read_matrix','write_vector','property','invoke','construct','text','intern','publish','clock','stable','scratch_vector','scratch_hit','refresh','unit']) ordnance_resources[name]=resource(name);
  return { bfe, camera_math: cameraMath, ordnance_resources };
}
export async function loadSim() {
  const holder = { bridge: null };
  const instance = await WebAssembly.instantiate(await simModule(), simImports(holder));
  instance.holder = holder;
  return instance;
}
