// The battle renderer is required; initialization errors stop loading.
import { loadRenderer } from "./failure.js";
export function loadRustRenderer(graphics) {
  return loadRenderer(graphics, async () => {
    const { createRustRender } = await import("./index.js");
    return createRustRender(graphics);
  });
}
