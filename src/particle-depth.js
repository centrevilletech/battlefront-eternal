import { drawFramebuffer } from "./render-targets.js";
import * as THREE from "three";

// Shared by particle batches; the renderer installs the capture callback.
export const particleDepth = {
  map: { value: null },
  enabled: { value: 0 },
  resolution: { value: new THREE.Vector2(1, 1) },
  cameraRange: { value: new THREE.Vector2(0.5, 200) },
  capture: () => {},
};

// Snapshot opaque depth once, immediately before the first visible particle.
// Depth-only MSAA resolve: no second geometry pass and no texture feedback loop.
export class ParticleDepth {
  constructor(renderer) {
    this.renderer = renderer;
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      depthTexture: new THREE.DepthTexture(1, 1, THREE.UnsignedIntType),
    });
    particleDepth.map.value = this.target.depthTexture;
    particleDepth.capture = (renderer, scene, camera, required = false) => this.capture(camera, required);
    this.pending = false;
    this.copies = 0;
  }

  begin(source, camera, enabled) {
    this.source = source;
    this.pending = true;
    this.enabled = enabled;
    this.copies = 0;
    particleDepth.enabled.value = enabled ? 1 : 0;
    particleDepth.cameraRange.value.set(camera.near, camera.far);
    particleDepth.resolution.value.set(source.width, source.height);
    this.target.setSize(source.width, source.height);
  }

  prepare(width, height) {
    this.target.setSize(width, height);
    this.renderer.initRenderTarget(this.target);
  }

  capture(camera, required = false) {
    const renderer = this.renderer;
    if (renderer.getRenderTarget() !== this.source || (!this.enabled && !required)) return false;
    if (!this.pending) return true;
    this.pending = false;
    const gl = renderer.getContext();
    const sourceFramebuffer = drawFramebuffer(renderer, this.source);
    const { width, height } = this.source;
    // setRenderTarget initializes the texture and destination framebuffer.
    // Bind through Three's state cache so the next draw restores correctly.
    renderer.setRenderTarget(this.target);
    // r170 tracks READ separately but FRAMEBUFFER binds change it too.
    // Invalidate that cache entry before selecting the source for the blit.
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER, sourceFramebuffer);
    gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.DEPTH_BUFFER_BIT, gl.NEAREST);
    renderer.setRenderTarget(this.source);
    this.copies++;
    return true;
  }
}
