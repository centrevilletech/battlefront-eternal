import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { CascadeWindow, planCascades } from "../src/shadow-cascades.js";

const box = (minX, minY, minZ, maxX, maxY, maxZ) => new THREE.Box3(new THREE.Vector3(minX, minY, minZ), new THREE.Vector3(maxX, maxY, maxZ));

test("a cascade window lands snapped to its texel grid and holds while the slice stays inside", () => {
  const w = new CascadeWindow();
  const texel = 0.25, half = 16, margin = 400;
  assert.equal(w.fit(box(-10.3, -6.1, -5, 10.3, 6.1, 5), half, texel, margin), true);
  assert.equal(w.held, true);
  assert.equal(w.dirty, true);
  assert.equal(w.moved, true);
  // Snapped: the centre is a whole number of texels from the origin.
  assert.equal(Math.abs(w.center.x / texel - Math.round(w.center.x / texel)) < 1e-9, true);
  assert.equal(Math.abs(w.center.y / texel - Math.round(w.center.y / texel)) < 1e-9, true);
  assert.equal(w.center.z, 5 + margin);
  const landed = w.center.clone();
  w.dirty = false; w.moved = false;
  // The viewer walks 4 m: the slice is still inside the window.
  assert.equal(w.fit(box(-6.3, -6.1, -5, 14.3, 6.1, 5), half, texel, margin), false);
  assert.deepEqual(w.center.toArray(), landed.toArray());
  assert.equal(w.dirty, false);
  // Past the edge: it lands again.
  assert.equal(w.fit(box(-2, -6.1, -5, 18.5, 6.1, 5), half, texel, margin), true);
  assert.notDeepEqual(w.center.toArray(), landed.toArray());
  assert.equal(w.dirty, true);
});

test("a window moves again when it would lose half its margin toward the sun, or its size changes", () => {
  const w = new CascadeWindow();
  w.fit(box(-1, -1, -1, 1, 1, 1), 8, 0.1, 400);
  w.dirty = false; w.moved = false;
  assert.equal(w.fit(box(-1, -1, -1, 1, 1, 180), 8, 0.1, 400), false);
  assert.equal(w.fit(box(-1, -1, -1, 1, 1, 220), 8, 0.1, 400), true);
  w.dirty = false; w.moved = false;
  assert.equal(w.fit(box(-1, -1, -1, 1, 1, 1), 9, 0.1, 400), true, "a new projection re-lands");
  w.invalidate();
  assert.equal(w.held, false);
  assert.equal(w.fit(box(-1, -1, -1, 1, 1, 1), 9, 0.1, 400), true);
});

test("the pass plan renders static casters only when a window lands, and alternates the last cascade's moving casters", () => {
  const windows = Array.from({ length: 4 }, () => new CascadeWindow());
  for (const w of windows) w.fit(box(-1, -1, -1, 1, 1, 1), 8, 0.1, 400);
  let plan = planCascades(windows, 0);
  assert.deepEqual(plan.map((p) => p.static), [true, true, true, true]);
  assert.deepEqual(plan.map((p) => p.dynamic), [true, true, true, true]);
  for (const w of windows) { w.dirty = false; w.moved = false; }
  plan = planCascades(windows, 1);
  assert.deepEqual(plan.map((p) => p.static), [false, false, false, false]);
  assert.deepEqual(plan.map((p) => p.dynamic), [true, true, true, false]);
  plan = planCascades(windows, 2);
  assert.deepEqual(plan.map((p) => p.dynamic), [true, true, true, true]);
  // A window that just landed is always drawn in full that frame.
  windows[3].moved = true;
  assert.equal(planCascades(windows, 1)[3].dynamic, true);
  // Two cascades (the Low preset) never skip.
  const two = windows.slice(0, 2);
  assert.deepEqual(planCascades(two, 1).map((p) => p.dynamic), [true, true]);
  assert.deepEqual(planCascades(windows, 1, { alternateLast: false }).map((p) => p.dynamic), [true, true, true, true]);
});
