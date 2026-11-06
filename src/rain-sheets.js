import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// Enhanced rain's middle distance: three open cylinders around the eye (4, 9
// and 18 m) scrolling a streak texture down and across with the wind, as in
// CryEngine's and Lagarde's rain. Individual drops (weather.js) stay near the
// camera; these give heavy rain its density and its sheets in the distance
// at a fixed cost: one draw, at most three thin layers. They keep to the
// world's axes, so turning doesn't drag them, stop under roofs (the drops'
// cover map), fade into geometry and take the scene's light, lightning
// included.
const RADII = [4, 9, 18], SEGMENTS = 48, ABOVE = 16, BELOW = 10;

// A tileable sheet of falling streaks, thin, varied in length and strength,
// pre-blurred across so they stay soft when minified; its texels are made
// by the Rust phase (weather.rs streaks).
function streakTexture(data, size = 256) {
  const texture = new THREE.DataTexture(data, size, size);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  return texture;
}

function cylinders() {
  const sheet = [], index = [];
  RADII.forEach((radius, layer) => {
    const base = sheet.length / 3;
    for (let j = 0; j <= SEGMENTS; j++) for (const k of [0, 1]) sheet.push(layer, j / SEGMENTS, k);
    for (let j = 0; j < SEGMENTS; j++) {
      const a = base + j * 2;
      index.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
    }
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('sheet', new THREE.Float32BufferAttribute(sheet, 3));
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(sheet.length), 3));
  geometry.setIndex(index);
  return geometry;
}



export class RainSheets {
  constructor(scene, uniforms, streaks) {
    this.texture = streakTexture(streaks);
    this.material = ownedEffectMaterial("effects/rain-sheets", {
      transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true,
      uniforms: {
        streaks: { value: this.texture }, eye: uniforms.eye, rainTime: uniforms.rainTime, wind: uniforms.wind, color: uniforms.color,
        speed: { value: Math.max(6, uniforms.velocity.value * 1.4) }, radii: { value: new THREE.Vector3(...RADII) }, strength: { value: 1 },
        cover: uniforms.cover, coverCell: uniforms.coverCell, coverSize: uniforms.coverSize,
        sceneDepth: particleDepth.map, softParticles: particleDepth.enabled, depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
        ambient: lighting.ambient,
      },
    });
    this.mesh = new THREE.Mesh(cylinders(), this.material);
    this.mesh.name = 'rain-sheets';
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 19;
    this.mesh.visible = false;
    this.mesh.onBeforeRender = (renderer, scene, camera) => particleDepth.capture(renderer, scene, camera);
    scene.add(this.mesh);
  }
  dispose() { this.mesh.removeFromParent(); this.mesh.geometry.dispose(); this.material.dispose(); this.texture.dispose(); }
}
