import { SurfaceOwner, stepSurface } from '../helpers/surfaces.js';
// A scripted flight through the map's haze for the Rust atmosphere phase to
// be held to: a FogCloud and two dusteffect classes over a terrain with a
// hole in it, a camera that moves, turns and changes its field of view, and
// frames of every length the page sees (including none and long ones). One
// row of digests per frame: each square batch's count and a hash of the
// records the renderer reads, its camera axes and the statistics.
import * as THREE from 'three';
import { EFFECT } from '../../src/rust-render/effect-layout.js';

const FOG = { texture: 'mist', range: [90, 120], color: [168, 172, 180, 128], velocity: [5, 0.5], rotation: 0.1, height: 16, size: 28, density: 60, cell: 256 };
const DUST = (over = {}) => ({
  texture: 'dust', count: 25, minPos: [-50, 4, -50], maxPos: [-10, 12, -10], minVel: [2, 0, 2], maxVel: [10, 1, 10],
  minSize: 15, maxSize: 20, minLifetime: 3, maxLifetime: 5, alpha: 0.5, minDistance: 0, maxDistance: 150, radiusFade: [50, 60], heightScale: 0.2, radius: 70, ...over,
});
export const ATMOSPHERE = {
  fogCloud: FOG,
  dust: {
    classes: { low: DUST(), far: DUST({ count: 40, minDistance: 40, maxDistance: 400, radius: 30, radiusFade: [0, 0], heightScale: 1, alpha: 0.8 }) },
    instances: [
      ...[0, 1, 2, 3].map((i) => ({ class: 'low', position: [i * 60 - 90, 3, 40 + i * 25] })),
      ...[0, 1, 2, 3, 4, 5].map((i) => ({ class: 'far', position: [i * 90 - 200, 8, -120 + i * 30] })),
    ],
  },
};

// Ground that is a hill and a hole (no ground under it).
export const collision = {
  ground(x, z, top, out) {
    if (x > 40 && x < 90 && z > 10 && z < 50) return null;
    out.height = 3 + 0.01 * x - 0.02 * z + Math.sin(x * 0.05) * 2;
    return out;
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

export function runAtmosphere(Atmosphere) {
  const scene = new THREE.Scene();
  const atmosphere = new Atmosphere({surfaceOwner:new SurfaceOwner(), data: ATMOSPHERE, scene, collision, texture: (name) => ({ name }) });
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.1, 2000);
  // The module's memory must never move: the views the page took stay good.
  const memory = atmosphere.host.x.memory.buffer;
  const rows = [];
  let t = 0;
  for (let frame = 0; frame < 400; frame++) {
    const dt = frame % 53 === 0 ? 0 : frame % 41 === 0 ? 0.6 : frame % 3 ? 1 / 60 : 1 / 30;
    t += dt;
    camera.position.set(Math.sin(t * 0.3) * 80, 6 + Math.sin(t) * 3, -40 + t * 4);
    camera.rotation.set(Math.sin(t * 0.2) * 0.2, t * 0.4, 0, 'YXZ');
    camera.fov = frame < 200 ? 60 : 85;
    camera.aspect = frame < 300 ? 16 / 9 : 4 / 3;
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    stepSurface(atmosphere,dt, camera);
    const row = [atmosphere.stats.fog, atmosphere.stats.dust, atmosphere.stats.objects];
    for (const batch of atmosphere.batches) {
      const n = batch.count;
      const u = batch.values;
      row.push(n, batch.mesh.visible ? 1 : 0, hash(hash(2166136261, batch.arrays.center, n * 4), batch.arrays.spin, n * 2),
        ...u.slice(EFFECT.SQUARE.AXIS_U, EFFECT.SQUARE.AXIS_U+3), ...u.slice(EFFECT.SQUARE.AXIS_V, EFFECT.SQUARE.AXIS_V+3));
    }
    // JSON keeps no sign for zero, so neither do the rows.
    rows.push(row.map((value) => value + 0));
  }
  if (atmosphere.host.x.memory.buffer !== memory) throw new Error('the module\'s memory moved');
  return rows;
}
