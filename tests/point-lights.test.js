import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { lightMask, pointLighting, sphereLightMask, useLights } from "../src/point-lights.js";

// Frozen packed inputs exercise draw selection independently of the native
// scene packer, whose full f64 producer fixtures live with bfe-render.
function fixturePack(){return {count:3,lights:[
  new THREE.Vector4(10,1,0,8),new THREE.Vector4(1,.21586050010324415,.051269458367115384,0),
  new THREE.Vector4(16,1,0,0),new THREE.Vector4(1,.21586050010324415,.051269458367115384,0),
  new THREE.Vector4(-20,1,5,5),new THREE.Vector4(1,.21586050010324415,.051269458367115384,0),
]};}

test("a draw reads the packed lights that reach its boxes or sphere, in order", () => {
  const pack=fixturePack();
  const box = (x0, z0, x1, z1) => new THREE.Box3(new THREE.Vector3(x0, 0, z0), new THREE.Vector3(x1, 5, z1));
  // Only the unbounded light reaches a box far from both bounded ones.
  assert.equal(lightMask(pack, [box(100, 100, 110, 110)]), 0b010);
  // The first light's sphere touches this box; a box not drawn doesn't count.
  assert.equal(lightMask(pack, [box(17, -1, 20, 1), box(-21, 4, -19, 6)], [1, 0]), 0b011);
  assert.equal(lightMask(pack, [box(17, -1, 20, 1), box(-21, 4, -19, 6)]), 0b111);
  assert.equal(sphereLightMask(pack, new THREE.Vector3(-22, 1, 5), 1), 0b110);
  useLights(pack, 0b101);
  assert.equal(pointLighting.drawLightCount.value, 2);
  // Flat, two vec4 a light, and only the lights in use go up.
  const drawn = pointLighting.drawLights.value;
  assert.equal(drawn.length, 16);
  assert.deepEqual([...drawn.subarray(0, 4)], [10, 1, 0, 8]);
  assert.deepEqual([...drawn.subarray(8, 12)], [-20, 1, 5, 5]);
  useLights(pack, 0);
  assert.equal(pointLighting.drawLightCount.value, 0);
  assert.equal(pointLighting.drawLights.value.length, 8);
});
