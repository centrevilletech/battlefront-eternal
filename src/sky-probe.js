import * as THREE from 'three';

// The map's own sky (its dome and the models on its horizon, drawn as the far
// scene draws them) in a small cube around the eye, for a sea that reflects
// it and fades into it far off (ocean-storm.js, on a sea it doesn't storm
// over). The sky stands around the eye, so a capture holds as the eye moves
// about; it is taken again once a second of the sea's time, or at once when
// the eye climbs or sinks 4 m (the horizon models' MovementScale), which also
// picks up scrolling sky textures and ones that arrive late. Only the sky's
// meshes are on its layer, so a capture draws nothing else.
export const PROBE_LAYER = 3; // graphics.js uses layers 1 (hidden) and 2 (static)

export class SkyProbe {
  constructor(graphics, size = 128) {
    this.graphics = graphics;
    this.target = new THREE.WebGLCubeRenderTarget(size, { generateMipmaps: true, minFilter: THREE.LinearMipmapLinearFilter });
    this.texture = this.target.texture;
    this.camera = new THREE.CubeCamera(0.05, 50, this.target);
    for (const face of this.camera.children) face.layers.set(PROBE_LAYER);
    this.layers = graphics.skyLayers?.length ? graphics.skyLayers : graphics.sky ? [{ mesh: graphics.sky, follow: null }] : [];
    for (const { mesh } of this.layers) mesh.layers.enable(PROBE_LAYER);
    this.enabled = false;
    this.last = -Infinity;
    this.eye = new THREE.Vector3(0, Infinity, 0);
    this.captures = 0;
  }

  update(time, camera) {
    const renderer = this.graphics.renderer;
    if (!this.enabled || !renderer || !this.layers.length) return;
    const eye = camera.position;
    if (time - this.last < 1 && Math.abs(eye.y - this.eye.y) < 4) return;
    this.last = time;
    this.eye.copy(eye);
    this.captures++;
    // The sky as graphics.render() stands it around this eye.
    for (const { mesh, follow } of this.layers) {
      mesh.position.copy(eye);
      if (follow) mesh.position.y = follow[0] * eye.y + (1 - follow[0]) * follow[1];
    }
    this.camera.position.copy(eye);
    this.camera.updateMatrixWorld();
    const autoClear = renderer.autoClear, shadows = renderer.shadowMap.autoUpdate;
    try {
      renderer.autoClear = true;
      renderer.shadowMap.autoUpdate = false;
      this.camera.update(renderer, this.graphics.farScene);
    } finally {
      renderer.autoClear = autoClear;
      renderer.shadowMap.autoUpdate = shadows;
    }
  }

  dispose() {
    for (const { mesh } of this.layers) mesh.layers.disable(PROBE_LAYER);
    this.layers = [];
    this.target.dispose();
  }
}
