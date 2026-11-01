import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { lightMask, packPointLights, packed, pointLighting, sphereLightMask, useLights } from "../src/point-lights.js";

// A scene of point lights: on, off (the ordnance pool's idle ones), hidden,
// unbounded, and too far from the eye for anything drawn to be in reach.
function scene() {
  const s = new THREE.Scene();
  const add = (x, z, { range = 8, intensity = 1, visible = true, parent = s } = {}) => {
    const light = new THREE.PointLight(0xff8040, intensity, range, 1);
    light.position.set(x, 1, z); light.visible = visible; parent.add(light); return light;
  };
  add(10, 0);
  add(12, 0, { intensity: 0 });
  const group = new THREE.Group(); group.visible = false; s.add(group);
  add(14, 0, { parent: group });
  add(16, 0, { range: 0 });
  add(900, 0);
  add(-20, 5, { range: 5 });
  s.updateMatrixWorld(true);
  return s;
}

const camera = () => { const c = new THREE.PerspectiveCamera(60, 1, 0.5, 200); c.updateMatrixWorld(true); return c; };

test("only the lights that are on and in reach of what is drawn are packed, in scene order", () => {
  const s = scene(), eye = camera();
  const pack = packPointLights(s, eye);
  assert.equal(packed(s, eye), pack);
  assert.equal(pack.count, 3);
  assert.deepEqual(pack.lights[0].toArray(), [10, 1, 0, 8]);
  assert.deepEqual(pack.lights[2].toArray(), [16, 1, 0, 0]);
  assert.deepEqual(pack.lights[4].toArray(), [-20, 1, 5, 5]);
  // Colour times intensity, as three's own light uniforms have it.
  const c = new THREE.Color(0xff8040);
  assert.deepEqual(pack.lights[1].toArray().slice(0, 3), [c.r, c.g, c.b]);
  // An unpacked scene or camera has none.
  assert.equal(packed(new THREE.Scene(), eye).count, 0);
});

test("a draw reads the packed lights that reach its boxes or sphere, in order", () => {
  const s = scene(), eye = camera();
  const pack = packPointLights(s, eye);
  const box = (x0, z0, x1, z1) => new THREE.Box3(new THREE.Vector3(x0, 0, z0), new THREE.Vector3(x1, 5, z1));
  // Only the unbounded light reaches a box far from both bounded ones.
  assert.equal(lightMask(pack, [box(100, 100, 110, 110)]), 0b010);
  // The first light's sphere touches this box; a box not drawn doesn't count.
  assert.equal(lightMask(pack, [box(17, -1, 20, 1), box(-21, 4, -19, 6)], [1, 0]), 0b011);
  assert.equal(lightMask(pack, [box(17, -1, 20, 1), box(-21, 4, -19, 6)]), 0b111);
  assert.equal(sphereLightMask(pack, new THREE.Vector3(-22, 1, 5), 1), 0b110);
  useLights(pack, 0b101);
  assert.equal(pointLighting.drawLightCount.value, 2);
  assert.deepEqual(pointLighting.drawLights.value[0].toArray(), [10, 1, 0, 8]);
  assert.deepEqual(pointLighting.drawLights.value[2].toArray(), [-20, 1, 5, 5]);
});

test("a render made inside another leaves the outer one's lights alone", () => {
  const outer = scene(), eye = camera(), mirror = camera();
  const pack = packPointLights(outer, eye);
  // A volume's own scene, and the same scene from a mirror, mid-pass.
  packPointLights(new THREE.Scene(), eye);
  mirror.position.set(0, -1000, 0); mirror.updateMatrixWorld(true);
  packPointLights(outer, mirror);
  assert.equal(packed(outer, eye), pack);
  assert.equal(pack.count, 3);
  assert.equal(packed(outer, mirror).count, 1);
});
