import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Atmosphere, DustCloud, EngineRandom, GroundHeights, dustFade, fogCloudParticles } from '../src/atmosphere.js';

// Geonosis's haze as the importer writes it (tools/zero_atmosphere.py).
const FOG = { texture: 'brownmist', range: [90, 120], color: [168, 172, 180, 128], velocity: [5, 0], rotation: 0.1, height: 16, size: 28, density: 60, cell: 256 };
const DUST = {
  texture: 'browndust', count: 25, minPos: [-50, 4, -50], maxPos: [-10, 12, -10], minVel: [2, 0, 2], maxVel: [10, 1, 10],
  minSize: 15, maxSize: 20, minLifetime: 3, maxLifetime: 5, alpha: 0.5, minDistance: 0, maxDistance: 150, radiusFade: [50, 60], heightScale: 0.2, radius: 70,
};
const flat = () => 0;
const view = (eye, forward, tan = Math.tan(Math.PI / 6)) => {
  const f = new THREE.Vector3(...forward).normalize(), right = new THREE.Vector3().crossVectors(f, new THREE.Vector3(0, 1, 0)).normalize();
  return { eye: new THREE.Vector3(...eye), forward: f, right, up: new THREE.Vector3().crossVectors(right, f), tanX: tan * 16 / 9, tanY: tan };
};
const collect = (def, time, v, heightAt = flat) => {
  const points = [];
  fogCloudParticles(def, time, v, heightAt, (x, y, z, angle, alpha) => points.push({ x, y, z, angle, alpha }));
  return points;
};

test('the engine random numbers: two LCG steps a draw, the seed xored first', () => {
  // The same rule written out: s1 = s*1664525+1013904223, s2 likewise from
  // s1, the draw the high half of s2 over the high half of s1.
  let s = (1 ^ 0xc71c2830) >>> 0;
  const reference = () => {
    const s1 = (Math.imul(s, 1664525) + 1013904223) >>> 0, s2 = (Math.imul(s1, 1664525) + 1013904223) >>> 0;
    s = s2; return ((s2 & 0xffff0000) | (s1 >>> 16)) >>> 0;
  };
  const random = new EngineRandom(1);
  for (let i = 0; i < 8; i++) assert.equal(random.int(), reference());
  const floats = Array.from({ length: 1000 }, () => random.float());
  assert.ok(floats.every(f => f >= 0 && f < 1));
  assert.ok(Math.abs(floats.reduce((a, b) => a + b) / floats.length - 0.5) < 0.05);
});

test('FogCloud stands its squares in the range band over the ground, faded at both ends', () => {
  const v = view([10, 2, 10], [1, 0, 0.3]);
  const points = collect(FOG, 0, v, (x, z) => 0.01 * x - 0.02 * z);
  assert.ok(points.length > 0 && points.length < 200, `${points.length} squares`);
  for (const p of points) {
    const d = Math.hypot(p.x - 10, p.y - 2, p.z - 10);
    assert.ok(d > FOG.range[0] && d < FOG.range[1], `distance ${d}`);
    assert.ok(Math.abs(p.y - (0.01 * p.x - 0.02 * p.z) - FOG.height) < 1e-9, 'height over the ground under it');
    const t = (d * d - (90 * 90 + 120 * 120) / 2) * 2 / (120 * 120 - 90 * 90);
    assert.ok(Math.abs(p.alpha - (1 - t * t) * 128 / 255) < 1e-9);
    assert.ok(p.angle >= -Math.PI && p.angle <= Math.PI);
  }
  // Nothing behind the camera: the band is the view's slab.
  assert.ok(points.every(p => (p.x - 10) * v.forward.x + (p.z - 10) * v.forward.z > -FOG.size));
  // No ground under a point: it isn't drawn.
  assert.equal(collect(FOG, 0, v, () => null).length, 0);
});

test('FogCloud repeats every 256 m and drifts with the wind', () => {
  const v = view([0, 2, 0], [0, 0, 1]);
  const at = collect(FOG, 0, v).map(p => [p.x, p.z]);
  // A full cell of wind later the pattern is back where it was.
  const later = collect({ ...FOG, rotation: 0 }, 256 / 5, v).map(p => [p.x, p.z]);
  const key = ([x, z]) => `${x.toFixed(3)},${z.toFixed(3)}`;
  assert.deepEqual(new Set(later.map(key)), new Set(at.map(key)));
  // A second on, every point has moved 5 m along x (those still in the band).
  const before = collect(FOG, 1, v), after = collect(FOG, 2, v);
  const moved = after.filter(p => before.some(q => Math.abs(q.x + 5 - p.x) < 1e-6 && Math.abs(q.z - p.z) < 1e-6));
  assert.ok(moved.length >= after.length - 4, `${moved.length} of ${after.length} moved with the wind`);
  // The same seed every frame: the pattern doesn't shimmer.
  assert.deepEqual(collect(FOG, 3.25, v), collect(FOG, 3.25, v));
});

test('dust particles start in the box at full alpha, move, respawn, and die past the radius fade', () => {
  const cloud = new DustCloud(DUST, new EngineRandom(7));
  assert.equal(cloud.particles.length, 25);
  for (const p of cloud.particles) {
    for (let k = 0; k < 3; k++) assert.ok(p[k] >= DUST.minPos[k] && p[k] <= DUST.maxPos[k]);
    assert.ok(p[6] >= 15 && p[6] <= 20 && p[7] >= 0 && p[7] <= 5 && p[9] === 0.5);
  }
  const p = cloud.particles[0], x = p[0], left = p[7];
  cloud.update(0.01);
  if (left > 0.01) assert.ok(Math.abs(p[0] - (x + p[3] * 0.01)) < 1e-4);
  for (let i = 0; i < 600; i++) {
    cloud.update(1 / 60);
    for (const q of cloud.particles) {
      // A particle can start outside it, unseen until the next update
      // starts it again (the engine's own rule).
      if (q[9] > 0) assert.ok(Math.hypot(q[0], q[2]) <= 60 + 1e-3, 'past RadiusFadeMax a particle starts again');
      assert.ok(q[9] >= 0 && q[9] <= 0.5 + 1e-6);
    }
  }
  // Alpha rises at 1 a second to Alpha; in its last second it is Alpha x
  // life left x that.
  const fresh = new DustCloud({ ...DUST, radiusFade: [-1, -1], count: 1 }, new EngineRandom(3));
  const q = fresh.particles[0];
  fresh.spawn(q); q[7] = 4;
  fresh.update(0.25); assert.ok(Math.abs(q[9] - 0.25) < 1e-6);
  fresh.update(0.5); assert.ok(Math.abs(q[9] - 0.5) < 1e-6);
  q[7] = 0.6; fresh.update(0.1); assert.ok(Math.abs(q[9] - 0.5 * 0.5 * 0.5) < 1e-6);
});

test('a dust object fades by its depth plus the class radius', () => {
  assert.equal(dustFade(DUST, -80), 0, 'wholly behind the camera');
  assert.equal(dustFade(DUST, 81), 0, 'past MaxDistance');
  assert.ok(Math.abs(dustFade(DUST, 0) - 0.4) < 1e-9);
  assert.ok(Math.abs(dustFade(DUST, -70) - 0.75) < 1e-9);
  assert.equal(dustFade({ ...DUST, maxDistance: 1000 }, 0), 1);
  assert.ok(Math.abs(dustFade({ ...DUST, minDistance: 100 }, 80) - 0.5) < 1e-9, 'fades in over 100 m past MinDistance');
});

test('ground heights are asked once per corner and blended', () => {
  let asked = 0;
  const collision = { ground(x, z, top, out) { asked++; if (x > 1000) return null; out.height = x * 0.5 + z; return out; } };
  const heights = new GroundHeights(collision);
  assert.equal(heights.at(4, 4), 6);
  assert.equal(asked, 4);
  for (let i = 0; i < 100; i++) heights.at(4 + i * 0.01, 4);
  assert.equal(asked, 4);
  assert.equal(heights.at(1004, 0), 500, 'the corners with ground at an edge');
  assert.equal(heights.at(2000, 0), null);
});

const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
const geonosis = {
  fogCloud: FOG,
  dust: { classes: { geo_prop_dust: DUST }, instances: [{ class: 'geo_prop_dust', position: [30, 0, 0] }, { class: 'geo_prop_dust', position: [5000, 0, 0] }] },
};
const flatCollision = { ground(x, z, top, out) { out.height = 0; return out; } };

test('the map haze builds one bounded draw per texture and fills it from the camera', () => {
  const scene = new THREE.Scene();
  const atmosphere = new Atmosphere({ data: geonosis, scene, collision: flatCollision, texture: () => texture });
  assert.deepEqual(scene.children.map(m => m.name), ['map-fog-cloud', 'map-dust-geo_prop_dust']);
  const camera = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 2000);
  camera.position.set(0, 2, 0); camera.lookAt(100, 2, 0);
  for (let i = 0; i < 30; i++) atmosphere.update(1 / 30, camera);
  const [fog, dust] = atmosphere.batches;
  assert.ok(fog.geometry.instanceCount > 0 && fog.mesh.visible);
  assert.equal(atmosphere.stats.objects, 1, 'the far copy is past its MaxDistance');
  assert.equal(dust.geometry.instanceCount, atmosphere.stats.dust);
  assert.ok(dust.geometry.instanceCount <= DUST.count);
  // Back to front: depths along the view never grow down the batch.
  const centers = dust.center.array, depths = [];
  for (let i = 0; i < dust.geometry.instanceCount; i++) depths.push(centers[i * 4]);
  assert.ok(depths.every((d, i) => i === 0 || d <= depths[i - 1] + 1e-4));
  // Flattened by the height scale: centres sit within 0.2 x 12 m of the copy.
  for (let i = 0; i < dust.geometry.instanceCount; i++) assert.ok(centers[i * 4 + 1] >= 0.8 - 1e-4 && centers[i * 4 + 1] <= 2.4 + 0.2 * 1 * 5 + 1e-3);
  assert.ok(Math.abs(dust.material.uniforms.axisU.value.y + 0.2) < 1e-6, 'the square is squashed vertically');
  // The band goes with the camera; the copies stay put.
  camera.position.set(0, 2, 3000); camera.lookAt(100, 2, 3000);
  atmosphere.update(1 / 30, camera);
  assert.ok(fog.geometry.instanceCount > 0);
  assert.equal(atmosphere.stats.objects, 0);
  assert.equal(dust.geometry.instanceCount, 0);
  assert.equal(dust.mesh.visible, false);
  atmosphere.dispose();
  assert.equal(scene.children.length, 0);
});

test('a map without haze, or with a texture missing, draws nothing and costs nothing', () => {
  const scene = new THREE.Scene();
  const none = new Atmosphere({ data: null, scene, collision: flatCollision, texture: () => texture });
  none.update(1 / 60, new THREE.PerspectiveCamera());
  assert.equal(scene.children.length, 0);
  const missing = new Atmosphere({ data: geonosis, scene, collision: flatCollision, texture: () => null });
  assert.equal(scene.children.length, 0);
  missing.update(1 / 60, new THREE.PerspectiveCamera());
  missing.dispose(); none.dispose();
});

// Imported maps, where an extraction is present (tools/extract.py).
const HAZE = {
  geo1: { fog: 'brownmist', dust: { geo_prop_dust: 23 } },
  end1: { fog: 'fluffy', dust: {} },
  rhn2: { fog: 'fluffy', dust: { Rhn2_prop_cloud: 13 } },
  yav1: { fog: 'fluffy', dust: { yav_prop_mist: 23 } },
  yav2: { fog: 'fluffy', dust: { yav_prop_mist: 9 } },
  hot1: { fog: null, dust: { hoth_prop_dust: 52, hoth_prop_mist: 15 } },
  kam1: { fog: null, dust: { kam1_prop_mist: 22 } },
  tat1: { fog: null, dust: { tat_prop_dust: 66 } },
  bes2: null, nab2: null, tat2: null,
};
for (const [map, expected] of Object.entries(HAZE)) {
  const path = new URL(`../public/maps/${map}/map.json`, import.meta.url);
  const data = fs.existsSync(path) ? JSON.parse(fs.readFileSync(path)) : null;
  test(`${map}: its imported haze builds`, { skip: !data || !('atmosphere' in data) }, () => {
    if (!expected) return assert.equal(data.atmosphere, null);
    assert.equal(data.atmosphere.fogCloud?.texture ?? null, expected.fog);
    const counts = {};
    for (const i of data.atmosphere.dust?.instances || []) counts[i.class] = (counts[i.class] || 0) + 1;
    assert.deepEqual(counts, expected.dust);
    for (const name of [expected.fog, ...Object.values(data.atmosphere.dust?.classes || {}).map(c => c.texture)].filter(Boolean))
      assert.ok(data.textures[name], `${name} exported`);
    const scene = new THREE.Scene();
    const atmosphere = new Atmosphere({ data: data.atmosphere, scene, collision: flatCollision, texture: () => texture });
    assert.equal(scene.children.length, (expected.fog ? 1 : 0) + Object.keys(expected.dust).length);
    atmosphere.dispose();
  });
}
