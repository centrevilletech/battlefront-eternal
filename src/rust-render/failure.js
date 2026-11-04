// Include the driver's device name in renderer failures so a loading or
// shader error can be reported without exposing shader source to the player.
export function rendererFailure(renderer, cause) {
  let gpu = "unknown GPU";
  try {
    const gl = renderer.getContext();
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    gpu = (info && gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) || gl.getParameter(gl.RENDERER) || gpu;
  } catch {}
  return new Error(`Battle renderer failed on ${gpu}: ${cause?.message || String(cause)}`, { cause });
}

export async function loadRenderer(graphics, load) {
  try {
    return await load(graphics);
  } catch (cause) {
    throw rendererFailure(graphics.renderer, cause);
  }
}
