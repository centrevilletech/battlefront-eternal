// The battle renderer is required; initialization errors stop loading.
import { loadRenderer } from "./failure.js";
// Resolve the module before battle preparation starts; forwarding rejection
// keeps the loading screen's GPU-labelled cause intact.
export function loadRustRenderer(graphics) {
  return loadRenderer(graphics, async () => {
    const { createRustRender } = await import("./index.js");
    return createRustRender(graphics);
  });
}
