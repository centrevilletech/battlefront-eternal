import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { ParticleDepth, particleDepth } from "../src/particle-depth.js";

// three's transparent list of a scene as r170's projectObject builds it and
// its sort orders it: each shown mesh whose material is shown, keyed by its
// group order, render order, z (its bounding sphere's centre through its
// world matrix and the view-projection, no divide) and id.
function threeOrder(scene, camera, depth) {
  const screen = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
  const items = [];
  const walk = (o, groupOrder) => {
    if (!o.visible) return;
    if (o.isGroup) groupOrder = o.renderOrder;
    else if (o.isMesh && o.material.visible) {
      const g = o.geometry;
      if (g.boundingSphere === null) g.computeBoundingSphere();
      const v = new THREE.Vector4().copy(g.boundingSphere.center).applyMatrix4(o.matrixWorld).applyMatrix4(screen);
      items.push({ object: o, groupOrder, renderOrder: o.renderOrder, z: v.z, id: o.id });
    }
    for (const child of o.children) walk(child, groupOrder);
  };
  walk(scene, 0);
  if (depth?.copier.visible) {
    const v = new THREE.Vector4().setFromMatrixPosition(depth.copier.matrixWorld).applyMatrix4(screen);
    items.push({ object: depth.copier, groupOrder: depth.group.renderOrder, renderOrder: depth.copier.renderOrder, z: v.z, id: -1 });
  }
  items.sort((a, b) => (a.groupOrder !== b.groupOrder ? a.groupOrder - b.groupOrder : a.renderOrder !== b.renderOrder ? a.renderOrder - b.renderOrder : a.z !== b.z ? b.z - a.z : a.id - b.id));
  return items.map((item) => item.object);
}

// A reader's request key, as the effects pass makes it (effects.js keyOf).
function keyOf(mesh, groupOrder = 0) {
  if (mesh.geometry.boundingSphere === null) mesh.geometry.computeBoundingSphere();
  const c = new THREE.Vector4().copy(mesh.geometry.boundingSphere.center).applyMatrix4(mesh.matrixWorld);
  return [groupOrder, mesh.renderOrder, c.x, c.y, c.z, mesh.id];
}

function setup() {
  const saved = { ...particleDepth }, map = particleDepth.map.value;
  const source = { name: "scene", width: 1600, height: 900 }, mirror = { name: "mirror", width: 512, height: 512 };
  let bound = source;
  const renderer = { getRenderTarget: () => bound, setRenderTarget: (t) => { bound = t; }, initRenderTarget() {} };
  const depth = new ParticleDepth(renderer);
  // The blit itself is WebGL's: counted.
  depth.copy = () => { depth.copies++; };
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(70, 16 / 9, 0.5, 2000);
  camera.position.set(3, 2, 10);
  camera.lookAt(0, 0, -10);
  camera.updateMatrixWorld();
  const reader = (x, y, z, renderOrder, parent = scene) => {
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.MeshBasicMaterial({ transparent: true }));
    mesh.position.set(x, y, z);
    mesh.renderOrder = renderOrder;
    parent.add(mesh);
    return mesh;
  };
  // A frame's render of the near scene as three starts it (graphics.js
  // begin(), then three's render: the scene's onBeforeRender, then its list).
  const start = (enabled, cam = camera, target = source) => {
    if (target === source) depth.begin(source, camera, enabled);
    bound = target;
    scene.updateMatrixWorld();
    scene.onBeforeRender(renderer, scene, cam, target);
  };
  const restore = () => { Object.assign(particleDepth, saved); particleDepth.map.value = map; };
  return { depth, renderer, scene, camera, source, mirror, reader, start, restore, bind: (t) => { bound = t; } };
}

test("the copier: just before the earliest reader asked for, in three's own order; a group's order its group's", () => {
  const { depth, scene, camera, reader, start, restore } = setup();
  try {
    // In three's order: far, between, near (render order 10, farther first), then late (12).
    const far = reader(0, 0, -40, 10), near = reader(0.5, 0, -6, 10), between = reader(-0.5, 0, -7, 10), late = reader(0, 0, -60, 12);
    let asks = [];
    particleDepth.requester(scene, () => { for (const [mesh, required] of asks) particleDepth.request(keyOf(mesh), required); });
    asks = [[late, true], [near, false]];
    start(true);
    assert.equal(depth.armed, true);
    const order = threeOrder(scene, camera, depth), at = order.indexOf(depth.copier);
    assert.deepEqual([order[at - 1], order[at + 1]], [between, near], "between the reader before it and the earliest asked for");
    const screen = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    const zOf = (o) => new THREE.Vector4().copy(o.geometry?.boundingSphere.center || new THREE.Vector3()).applyMatrix4(o.matrixWorld).applyMatrix4(screen).z;
    const gap = zOf(depth.copier) - zOf(near);
    assert.ok(gap > 0 && gap < 1e-12 * Math.max(1, Math.abs(zOf(near))), `a hair farther (${gap})`);
    assert.equal(order.indexOf(far), 0);
    assert.equal(order[order.length - 1], late);
    // Soft particles off: a reader that asks only while they are on asks for nothing.
    start(false);
    assert.equal(depth.armed, true, "the late reader asks whatever the setting");
    const off = threeOrder(scene, camera, depth);
    assert.deepEqual(off.slice(off.indexOf(depth.copier) - 1, off.indexOf(depth.copier) + 2), [near, depth.copier, late]);
    // A reader below a group: its group's order.
    const group = new THREE.Group();
    group.renderOrder = -5;
    scene.add(group);
    const grouped = reader(0, 1, -20, 3, group);
    asks = [];
    particleDepth.requester(scene, () => {
      particleDepth.request(keyOf(near), false);
      particleDepth.request(keyOf(grouped, -5), true);
    });
    start(true);
    const grouped0 = threeOrder(scene, camera, depth);
    assert.equal(depth.group.renderOrder, -5);
    assert.equal(depth.copier.renderOrder, 3);
    assert.deepEqual(grouped0.slice(0, 2), [depth.copier, grouped], "first, as the grouped reader is");
  } finally {
    restore();
  }
});

test("nothing placed without a request or for another render, and the copier shown once at first (its program built)", () => {
  const { depth, scene, camera, reader, start, source, mirror, restore } = setup();
  try {
    const mesh = reader(0, 0, -10, 10);
    let ask = false;
    particleDepth.requester(scene, () => { if (ask) particleDepth.request(keyOf(mesh), false); });
    start(true);
    assert.equal(depth.armed, false);
    assert.equal(depth.copier.visible, false, "no dummy draw is needed to compile a copy command");
    start(true);
    assert.equal(depth.copier.visible, false, "then hidden while nothing is asked for");
    ask = true;
    start(false);
    assert.equal(depth.copier.visible, false, "a soft reader with soft particles off");
    start(true);
    assert.equal(depth.copier.visible, true);
    assert.equal(depth.armed, true);
    // A mirror's render of the same scene: hidden for it (the frame's own
    // render had listed it already).
    start(true, new THREE.PerspectiveCamera(), mirror);
    assert.equal(depth.copier.visible, false);
    assert.equal(depth.armed, true, "still the frame's");
    // A request made outside a requester's call is not taken.
    assert.equal(particleDepth.request(keyOf(mesh), true), false);
    assert.equal(depth.source, source);
  } finally {
    restore();
  }
});

test("the copy: once a frame at the copier; readers ahead of their turn only learn it will be there; those not asked for and those that render at once take it at once", () => {
  const { depth, renderer, scene, camera, reader, start, mirror, restore, bind } = setup();
  try {
    const mesh = reader(0, 0, -10, 10);
    particleDepth.requester(scene, () => particleDepth.request(keyOf(mesh), false));
    const ahead = (requested, required = false) => { let got; particleDepth.ahead(() => { got = particleDepth.capture(renderer, scene, camera, required); }, requested); return got; };
    const early = () => { let got; particleDepth.early(() => { got = particleDepth.capture(renderer, null, camera, true); }); return got; };
    start(true);
    assert.equal(ahead(true), true, "the depth will be there");
    assert.equal(depth.copies, 0, "not taken yet");
    depth.capture(camera, true);
    assert.equal(depth.copies, 1, "taken at the copier");
    assert.equal(particleDepth.capture(renderer, scene, camera), true, "a reader of three's after it");
    assert.equal(depth.capture(camera, true), true, "the upscaler's");
    assert.equal(depth.copies, 1);
    // One not asked for (shown since): taken at once; the copier then has none to take.
    start(true);
    assert.equal(ahead(false), true);
    assert.equal(depth.copies, 1);
    depth.capture(camera, true);
    assert.equal(depth.copies, 1);
    // One that renders with it at once: a copy of its own (once a frame),
    // and the copier's still at its place.
    start(true);
    assert.equal(early(), true);
    assert.equal(early(), true);
    assert.equal(depth.copies, 1);
    depth.capture(camera, true);
    assert.equal(depth.copies, 2);
    // Nothing asked for: its copy the frame's.
    start(false);
    assert.equal(depth.armed, false);
    assert.equal(early(), true);
    assert.equal(depth.capture(camera, true), true);
    assert.equal(depth.copies, 1);
    // Off the frame's target (a mirror's render): nothing, whatever the mode.
    start(true);
    bind(mirror);
    assert.equal(ahead(true, true), false);
    assert.equal(early(), false);
    assert.equal(particleDepth.capture(renderer, scene, camera, true), false);
    assert.equal(depth.copies, 0);
  } finally {
    restore();
  }
});
