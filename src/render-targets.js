// Three r170's framebuffer bookkeeping, shared by mid-pass color/depth copies.
// Reading FRAMEBUFFER_BINDING from WebGL can synchronize the CPU with the GPU.
// These are initialized 2D targets currently being drawn by the renderer; use
// its existing handles, including the render-to-texture MSAA extension path.
export function drawFramebuffer(renderer, target) {
  const properties = renderer.properties.get(target);
  return target.samples > 0 && properties.__webglMultisampledFramebuffer
    ? properties.__webglMultisampledFramebuffer
    : properties.__webglFramebuffer;
}
