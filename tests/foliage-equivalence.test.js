// Pre-optimization card equations retained as the vertex/visibility oracle.
import assert from 'node:assert/strict';
import test from 'node:test';
import * as THREE from 'three';
import { Foliage } from '../src/foliage.js';
const GRASS_FADE = 20, LEAF_FADE = 0.4;
const GRASS_TURNS = [0, -.05, .1, -.15, .2, 0, .05, -.1, .15, -.2];
const LEAF_TURNS = [0, -1, 2, -3, 4, 0, 1, -2, 3, -4];
const ATLAS = [[0,.5,.5,1],[.5,.5,1,1],[0,0,.5,.5],[.5,0,1,.5]], WHOLE = [0,0,1,1];
function drawGrass(kind, dt, camera, right, back) {
    const def = kind.def;
    const maxDist = def.MaxDistance ?? 100;
    const alpha = def.Alpha ?? 0.5;
    for (const card of kind.cards) card.phase += card.rate * dt;
    // Up leans toward the camera; flat toward it too.
    const lean = this.v[3].set(0, 1, 0).addScaledVector(back, -0.7 * back.y).normalize();
    const flat = this.v[4].set(-back.x, 0, -back.z);
    if (flat.lengthSq() < 1e-8) flat.set(1, 0, 0);
    flat.normalize();
    const [bl, br, tl, tr, wd, at] = [this.v[5], this.v[6], this.v[7], this.v[8], this.v[9], new THREE.Vector3()];
    const camPos = camera.position;
    const view = this.v[2];
    for (const inst of kind.instances) {
      // Depth along the view.
      const depth = -at.subVectors(inst.position, camPos).dot(view);
      if (depth > maxDist || depth < -5) continue;
      const a = depth <= maxDist - GRASS_FADE ? 1 : (maxDist - depth) / GRASS_FADE;
      const sink = 1 - (1 - a) ** 2;
      kind.cards.forEach((card, i) => {
        const k = GRASS_TURNS[i % 10];
        wd.copy(right).multiplyScalar(1.1 * (1 - Math.abs(k))).addScaledVector(back, 1.1 * k).multiplyScalar(card.width);
        const base = at.set(inst.position.x + card.x, inst.position.y + card.y * sink, inst.position.z + card.z);
        bl.copy(base).addScaledVector(wd, -0.5);
        br.copy(bl).add(wd);
        tl.copy(base).addScaledVector(lean, card.height * sink).addScaledVector(wd, 0.08 * Math.sin(card.phase) - 0.5);
        tr.copy(tl).add(wd);
        const g = card.grey;
        kind.batch.push(bl, br, tl, tr, g, g, g, alpha, ATLAS[card.part], false);
      });
    }
  }

function drawLeaves(kind, dt, camera, right, up, back) {
    const def = kind.def;
    const maxDist = this.nearEnd();
    const alpha = def.Alpha ?? 0.5;
    const speed = def.WiggleSpeed ?? 1.2;
    const amount = def.WiggleAmount ?? 0.11;
    for (const leaf of kind.cards) leaf.phase += speed * dt;
    const [bl, br, tl, tr, ws, center] = [this.v[5], this.v[6], this.v[7], this.v[8], this.v[9], new THREE.Vector3()];
    const us = new THREE.Vector3();
    const camPos = camera.position;
    const scale = def.HeightScale ?? 1;
    const atlas = (def.NumParts ?? 4) === 4;
    for (const inst of kind.instances) {
      if (inst.sphere && !this.frustum.intersectsSphere(inst.sphere)) continue;
      const depth = -center.subVectors(inst.position, camPos).dot(back);
      if (depth > maxDist + 20 || depth < -25) continue;
      kind.cards.forEach((leaf, i) => {
        center.set(inst.position.x + leaf.p.x, inst.position.y + leaf.p.y * scale, inst.position.z + leaf.p.z).addScaledVector(back, 0.1);
        const d = -this.v[3].subVectors(center, camPos).dot(back);
        if (d > maxDist) return;
        const a = d <= (1 - LEAF_FADE) * maxDist ? 1 : (maxDist - d) / (LEAF_FADE * maxDist);
        const t = LEAF_TURNS[i % 10];
        ws.copy(right).addScaledVector(back, 0.3 * t).normalize().multiplyScalar(leaf.size);
        us.copy(up).multiplyScalar(leaf.size * scale);
        const sway = amount * Math.sin(leaf.phase);
        tl.copy(center).addScaledVector(us, 0.5).addScaledVector(ws, sway - 0.5);
        tr.copy(tl).add(ws);
        bl.copy(center).addScaledVector(us, -0.5).addScaledVector(ws, -0.5);
        br.copy(bl).add(ws);
        const g = leaf.grey;
        kind.batch.push(bl, br, tl, tr, g * inst.tint[0], g * inst.tint[1], g * inst.tint[2], alpha * a, atlas ? ATLAS[leaf.part] : WHOLE, !atlas && (leaf.part & 1) === 1);
      });
    }
  }

function build(data, nearEnd) {
  const original = Math.random;
  let seed = 775;
  Math.random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
  try { return new Foliage(new THREE.Scene(), data, { texture: () => new THREE.Texture(), nearEnd }); }
  finally { Math.random = original; }
}
test('shared foliage frames preserve every visible card, tint, atlas UV, sway, and distance fade', () => {
  const classes = {
    tree: { base: 'leafpatch', NumParticles: 60, Seed: 37, Radius: 5, Height: 9, HeightScale: 1.3, MinSize: .5, MaxSize: 1.2 },
    vine: { base: 'leafpatch', NumParticles: 45, Seed: 91, Vine: [3, 1], HeightScale: .8, MinSize: .3, MaxSize: .8, NumParts: 1 },
    grass: { base: 'grasspatch', NumParticles: 25, BoxSize: [4, 6], MinSize: .25, MaxSize: .5, MaxDistance: 90 },
  };
  const instances = [];
  for (const name of Object.keys(classes)) for (const z of [-210, -70, -25, 10]) for (const x of [-15, 0, 20]) {
    instances.push({ class: name, matrix: new THREE.Matrix4().makeTranslation(x, -12, z).toArray() });
  }
  let distance = 250;
  const expected = build({ classes, instances }, () => distance), actual = build({ classes, instances }, () => distance);
  expected.drawLeaves = drawLeaves; expected.drawGrass = drawGrass;
  const camera = new THREE.PerspectiveCamera(65, 1.6, .1, 600);
  camera.position.set(0, -9, 0);
  let visible = 0, maxDifference = 0;
  for (let frame = 0; frame < 96; frame++) {
    camera.rotation.set(Math.sin(frame * .13) * .6, frame * Math.PI / 48, 0);
    if (frame === 32) distance = 110;
    if (frame === 64) distance = 300;
    const dt = [0, 1 / 30, 1 / 60, 1 / 144][frame % 4];
    expected.update(dt, camera); actual.update(dt, camera);
    for (const [name, kind] of expected.kinds) {
      const a = actual.kinds.get(name).batch, b = kind.batch;
      assert.equal(a.count, b.count, `${name} culling frame ${frame}`); visible += a.count;
      for (const [attribute, stride] of [['positions', 12], ['colors', 16], ['uvs', 8]]) {
        for (let i = 0; i < a.count * stride; i++) {
          const delta = Math.abs(a[attribute][i] - b[attribute][i]);
          maxDifference = Math.max(maxDifference, delta);
          assert(delta <= 3e-5, `${name}.${attribute}[${i}] frame ${frame}: ${delta}`);
        }
      }
    }
  }
  assert(visible > 10000, 'exercise many visible translated cards');
  console.log(`Foliage equivalence: ${visible} cards, largest float32 difference ${maxDifference}`);
});
