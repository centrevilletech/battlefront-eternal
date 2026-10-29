import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { buildActor } from '../src/actor.js';
import { CrowdShadows } from '../src/crowd-shadows.js';

// A two-bone body of one triangle per bone, and a fake graphics that
// records casters like graphics.js does.
function graphicsStub() {
  const g = { scene: new THREE.Scene(), casters: new Set(), hiddenLayer: 1, csm: {} };
  g.caster = (o) => { g.casters.add(o); o.castShadow = true; return o; };
  return g;
}
const DEF = {
  bones: [
    { name: 'root', parent: -1, local: new THREE.Matrix4().toArray() },
    { name: 'hp_weapons', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 1, 0).toArray() },
  ],
  attach: 'hp_weapons',
  parts: [{ texture: 'body', vertexCount: 6, indexCount: 6, vertexOffset: 0, indexOffset: 0 }],
};
function vertices() {
  // position(3) normal(3) uv(2) bone(1) per vertex; the second triangle on bone 1.
  const v = [];
  for (let i = 0; i < 6; i++) v.push(i, i * 2, 0, 0, 1, 0, 0, 0, i < 3 ? 0 : 1);
  return new Float32Array(v).buffer;
}
const INDICES = new Uint32Array([0, 1, 2, 3, 4, 5]).buffer;

function unit(graphics, weapons = 1) {
  const actor = buildActor(DEF, vertices(), INDICES, () => null, graphics);
  graphics.scene.add(actor.root);
  const held = new Map();
  for (let w = 0; w < weapons; w++) {
    const group = new THREE.Group();
    const mesh = new THREE.Mesh(new THREE.BufferGeometry().setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0], 3)), new THREE.MeshBasicMaterial());
    mesh.material.name = 'zero:dynamic';
    mesh.geometry.setIndex([0, 1, 2]);
    graphics.caster(mesh);
    group.add(mesh);
    const flash = new THREE.Mesh(new THREE.BufferGeometry().setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0], 3)), new THREE.MeshBasicMaterial());
    group.add(flash); // not a casting part
    actor.hand.add(group);
    held.set(`gun${w}`, { group });
  }
  actor.root.visible = true;
  return { actor, combat: { held }, shadowNear: true, removed: false };
}

test('slots take consecutive bone and vertex ranges; proxies and guns stop casting', () => {
  const g = graphicsStub();
  const crowd = new CrowdShadows(g);
  const a = unit(g), b = unit(g, 2);
  const sa = crowd.add(a), sb = crowd.add(b);
  assert.equal(sa.boneBase, 0); assert.equal(sa.boneCount, 2);
  assert.equal(sb.boneBase, 3, 'a body, then one hand entry per weapon'); assert.equal(sb.weapons.length, 2);
  assert.equal(sa.vertexStart, 0); assert.equal(sa.vertexCount, 6 + 3);
  assert.equal(sb.vertexStart, 9); assert.equal(sb.vertexCount, 6 + 6);
  assert.equal(crowd.bones.length, 2 + 1 + 2 + 2);
  assert.equal(crowd.bones[2], a.actor.hand, 'a weapon entry is the hand bone again');
  assert.equal(a.actor.shadow.castShadow, false); assert.ok(!g.casters.has(a.actor.shadow));
  for (const w of sb.weapons) for (const m of w.meshes) { assert.equal(m.castShadow, false); assert.ok(!g.casters.has(m)); }
  assert.ok(g.casters.has(crowd.mesh), 'the crowd mesh casts instead');
  assert.equal(crowd.mesh.layers.mask, 1 << 1);
  // Skin indices point at the slot's bones.
  const skin = crowd.mesh.geometry.getAttribute('skinIndex');
  assert.equal(skin.getX(0), 0); assert.equal(skin.getX(5), 1);
  assert.equal(skin.getX(6), 2, "the gun's vertices follow its hand entry");
  assert.equal(skin.getX(9), 3); assert.equal(skin.getX(14), 3 + 1);
  assert.equal(skin.getX(15), 5); assert.equal(skin.getX(18), 6);
});

test('only shown bodies and held weapons are packed into the drawn indices', () => {
  const g = graphicsStub();
  const crowd = new CrowdShadows(g);
  const a = unit(g), b = unit(g, 2);
  crowd.add(a); crowd.add(b);
  b.combat.held.get('gun1').group.visible = false;
  crowd.update();
  assert.equal(crowd.draws, 2);
  assert.equal(crowd.drawnIndices, (6 + 3) + (6 + 3), 'the hidden gun is left out');
  const inv = crowd.skeleton.boneInverses;
  assert.deepEqual(inv[0].elements, a.actor.skeleton.boneInverses[0].elements);
  assert.deepEqual(inv[2].elements, new THREE.Matrix4().elements, 'a held weapon skins with the identity');
  assert.ok(inv[6].elements.every((e) => e === 0), 'an unheld one collapses');
  // Beyond shadow range: the slot leaves the drawn set and its bones collapse.
  const drawnBefore = crowd.mesh.geometry.index.version;
  a.shadowNear = false;
  crowd.update();
  assert.equal(crowd.draws, 1);
  assert.equal(crowd.drawnIndices, 6 + 3);
  assert.ok(inv[0].elements.every((e) => e === 0));
  assert.ok(crowd.mesh.geometry.index.version > drawnBefore, 'the index buffer re-uploads when the set changes');
  const version = crowd.mesh.geometry.index.version;
  crowd.update();
  assert.equal(crowd.mesh.geometry.index.version, version, 'and not otherwise');
  // Nobody shown: the mesh stops casting.
  b.actor.root.visible = false;
  crowd.update();
  assert.equal(crowd.draws, 0);
  assert.equal(crowd.mesh.castShadow, false);
  b.actor.root.visible = true;
  crowd.update();
  assert.equal(crowd.mesh.castShadow, true);
});

test('growing past capacity rebuilds without moving earlier slots', () => {
  const g = graphicsStub();
  const crowd = new CrowdShadows(g);
  const first = unit(g);
  const slot = crowd.add(first);
  crowd.update();
  const mesh = crowd.mesh;
  const units = [];
  for (let i = 0; i < 200; i++) units.push(crowd.add(unit(g, 2)));
  assert.notEqual(crowd.mesh, mesh, 'a larger mesh replaced the first');
  assert.ok(!g.casters.has(mesh) && !mesh.parent, 'the old one is gone');
  assert.equal(slot.vertexStart, 0); assert.equal(slot.boneBase, 0);
  assert.equal(units[199].vertexStart, 9 + 199 * 12);
  assert.ok(crowd.vertexCapacity >= crowd.vertexCount && crowd.boneCapacity >= crowd.bones.length);
  crowd.update();
  assert.equal(crowd.draws, 201, 'every slot is shown again after the rebuild');
  assert.equal(crowd.drawnIndices, 9 + 200 * 12);
  const skin = crowd.mesh.geometry.getAttribute('skinIndex');
  assert.equal(skin.getX(units[199].vertexStart), units[199].boneBase);
  assert.equal(crowd.skeleton.bones.length, crowd.bones.length);
});

test('headless worlds and bodies without a hand register nothing', () => {
  const g = graphicsStub();
  const crowd = new CrowdShadows(g);
  assert.equal(crowd.add({ actor: null }), null);
  const u = unit(g, 1);
  u.actor.hand = null;
  const slot = crowd.add(u);
  assert.equal(slot.weapons.length, 0, 'no hand, no weapon entries');
});
