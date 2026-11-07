// The battle renderer is required; initialization errors stop loading.
import { loadRenderer } from "./failure.js";
let moduleReady;
// Resources and draw passes share one instance. Concurrent init() calls would
// replace bindgen's global memory while existing renderer pointers still live.
export function rendererModule() {
  return moduleReady ??= import("../rust-pkg/bfe_render.js").then(async module => ({ module, wasm: await (async()=>{
    // Headless worlds read native bind bounds for real body/radius queries.
    // Load the same cold compiler bytes before those actors are constructed;
    // no WebGL owner or drawing context is created on this path.
    if(import.meta.url.startsWith('file:')&&globalThis.process?.versions?.node){const specifier='node:fs/promises';const {readFile}=await import(/* @vite-ignore */ specifier);return module.default({module_or_path:await readFile(new URL('../rust-pkg/bfe_render_bg.wasm',import.meta.url))});}
    return module.default();
  })() }));
}
// Resolve the module before battle preparation starts; forwarding rejection
// keeps the loading screen's GPU-labelled cause intact.
export function loadRustRenderer(graphics) {
  return loadRenderer(graphics, async () => {
    const { createRustRender } = await import("./index.js");
    return createRustRender(graphics);
  });
}
