// Include the driver's device name in renderer failures so a loading or
// shader error can be reported without exposing shader source to the player.
export function rendererFailure(renderer, cause) {
  if (cause?.rendererFailure) return cause;
  // Driver details can be withheld by the browser. Keep the original cause
  // even when only its generic renderer string is available to identify it.
  let gpu = "unknown GPU";
  try {
    const gl = renderer ? renderer.getContext() : (typeof OffscreenCanvas === "function" ? new OffscreenCanvas(1,1).getContext("webgl2") : document.createElement("canvas").getContext("webgl2"));
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    gpu = (info && gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) || gl.getParameter(gl.RENDERER) || gpu;
  } catch {}
  const error = new Error(`Battle renderer failed on ${gpu}: ${cause?.message || String(cause)}`, { cause });
  error.rendererFailure = true;
  return error;
}

// Loading uses the same error format as link/rebuild failures. Rejection is
// preserved for the loading screen; success cannot select another drawer.
export async function loadRenderer(graphics, load) {
  try {
    return await load(graphics);
  } catch (cause) {
    throw rendererFailure(graphics.renderer, cause);
  }
}

// Three compiles material shells until shader ownership moves to Rust. Its
// diagnostic callback must stop loading too, rather than only logging a link.
export function requireLinkedPrograms(renderer) {
  // Three owns compilation of remaining material shells. Its default logging
  // would allow loading to finish with an unusable program, so throw here.
  renderer.debug.checkShaderErrors = true;
  renderer.debug.onShaderError = (gl, program) => {
    throw rendererFailure(renderer, new Error(`Shader program failed to link: ${gl.getProgramInfoLog(program) || "driver rejected program"}`));
  };
}
