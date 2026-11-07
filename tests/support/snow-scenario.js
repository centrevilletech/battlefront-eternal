import { SurfaceOwner, stepSurface } from '../helpers/surfaces.js';
// A scripted walk through falling snow for the Rust snow phase to be held to:
// Hoth's precipitation and gusting wind over a roofed and open ground, a
// camera that moves, turns and changes its lens, and frames of every length.
// One row of digests per frame: the draw's instance count and visibility, the
// statistics, the camera axes, and a hash of every record the shader reads
// (the flakes, the boxes and the cover cache).
import * as THREE from 'three';

export const HOTH = {
  precipitation: { Type: ['Quads'], Range: [15], Color: [255, 255, 255], Velocity: [2], VelocityRange: [0.8], ParticleDensity: [100],
    ParticleDensityRange: [0], ParticleSize: [0.015], AlphaMinMax: [0.3, 0.45], Texture: ['fx_snow'], RotationRange: [2] },
  wind: [4, 0.3], gusts: { range: 1, rate: 0.2 },
};

// Roofs over some of the ground, open sky over the rest.
export const collision = {
  cast(from, down, range, hit) {
    if (Math.sin(from.x * 0.2) + Math.cos(from.z * 0.15) > 0.8) return null;
    hit.point.set(from.x, 4 + Math.sin(from.x * 0.1), from.z);
    return hit;
  },
};

const bits = new Uint32Array(1);
const asFloat = new Float32Array(bits.buffer);
function hash(h, array, n) {
  for (let i = 0; i < n; i++) {
    asFloat[0] = array[i];
    h ^= bits[0];
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h;
}

export function runSnow(Snow) {
  const snow = new Snow({surfaceOwner:new SurfaceOwner(), data: HOTH, scene: new THREE.Scene(), collision, texture: () => ({}) });
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 300);
  // The module's memory must never move: the views the page took stay good.
  const memory = snow.host.x.memory.buffer;
  const rows = [];
  let t = 0;
  for (let frame = 0; frame < 300; frame++) {
    const dt = frame % 47 === 0 ? 0 : frame % 31 === 0 ? 0.5 : frame % 3 ? 1 / 60 : 1 / 30;
    t += dt;
    // A teleport now and then, to spend the cover cache's budget.
    camera.position.set(Math.sin(t * 0.3) * 20 + (frame > 150 ? 400 : 0), 1.8 + Math.sin(t) * 0.5, t * 2);
    camera.rotation.set(Math.sin(t * 0.2) * 0.3, t * 0.5, 0, 'YXZ');
    camera.fov = frame < 150 ? 60 : 100;
    camera.aspect = frame < 220 ? 16 / 9 : 1;
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    stepSurface(snow,dt, camera);
    const u = snow.material.uniforms;
    const n = snow.flakes?.count ?? snow.flakeCount;
    const row = [snow.geometry.instanceCount, snow.mesh.visible ? 1 : 0, snow.stats.boxes, snow.stats.flakes,
      ...u.cameraRight.value.toArray(), ...u.cameraUp.value.toArray(),
      hash(2166136261, snow.flakeData.image.data, n * 8), hash(2166136261, snow.boxData.image.data, snow.boxData.image.data.length),
      hash(2166136261, snow.cover.heights, snow.cover.heights.length), snow.cover.queries];
    rows.push(row.map((value) => value + 0));
    // (-0 and 0 are one number to JSON.)
  }
  if (snow.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
  return rows;
}
