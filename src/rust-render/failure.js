// Include the driver's device name in renderer failures so a loading or
// shader error can be reported without exposing shader source to the player.
export function rendererFailure(renderer, cause) {
  if (cause?.rendererFailure) return cause;
  let gpu = "unknown GPU";
  try {
    const gl = renderer.getContext();
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    gpu = (info && gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) || gl.getParameter(gl.RENDERER) || gpu;
  } catch {}
  const error = new Error(`Battle renderer failed on ${gpu}: ${cause?.message || String(cause)}`, { cause });
  error.rendererFailure = true;
  return error;
}

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
  renderer.debug.checkShaderErrors = true;
  renderer.debug.onShaderError = (gl, program) => {
    throw rendererFailure(renderer, new Error(`Shader program failed to link: ${gl.getProgramInfoLog(program) || "driver rejected program"}`));
  };
}
