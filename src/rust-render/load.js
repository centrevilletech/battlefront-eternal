// Keep recovery independent of the renderer module and its WebAssembly.
export async function loadRustRenderer(graphics, search = "") {
  if (new URLSearchParams(search).has("rustOff")) return null;
  const { createRustRender } = await import("./index.js");
  return createRustRender(graphics);
}
