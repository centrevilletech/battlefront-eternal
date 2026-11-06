// The battle renderer is required; initialization errors stop loading.
import { loadRenderer } from "./failure.js";
let moduleReady;
// Resources and draw passes share one instance. Concurrent init() calls would
// replace bindgen's global memory while existing renderer pointers still live.
export function rendererModule() {
  return moduleReady ??= import("../rust-pkg/bfe_render.js").then(async module => ({ module, wasm: await module.default() }));
}
// Resolve the module before battle preparation starts; forwarding rejection
// keeps the loading screen's GPU-labelled cause intact.
export function loadRustRenderer(graphics) {
  return loadRenderer(graphics, async () => {
    const { createRustRender } = await import("./index.js");
    return createRustRender(graphics);
  });
}
