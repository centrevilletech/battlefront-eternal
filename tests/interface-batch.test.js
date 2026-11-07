import { presentationModule } from "./helpers/page-presentation.js";
import { bindInterfaceBatch } from "../src/interface/batch.js";
bindInterfaceBatch(presentationModule());
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Batch } from '../src/interface/batch.js';

const texture = new THREE.Texture();
const M = new THREE.Matrix4();
const WHITE = [1, 1, 1, 1];
const ranges = (batch, name = 'position') => batch.geometry.getAttribute(name).updateRanges.map((r) => [r.start, r.count]);

function frame(batch, quads) {
  batch.begin();
  for (const [x, y] of quads) batch.quad(M, x, y, x + 10, y + 10, texture, [0, 0, 1, 1], WHITE);
  batch.end();
}

test('an unchanged frame uploads nothing; a changed quad uploads its own run', () => {
  const batch = new Batch();
  const quads = Array.from({ length: 40 }, (_, i) => [i * 20, 0]);
  frame(batch, quads);
  assert.equal(batch.uploads, 1, 'the first frame is one run');
  assert.deepEqual(ranges(batch), [[0, 40 * 4 * 3]]);
  assert.equal(batch.geometry.getAttribute('position').version, 1);
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  frame(batch, quads);
  assert.equal(batch.uploads, 0);
  assert.deepEqual(ranges(batch), []);
  assert.equal(batch.geometry.getAttribute('position').version, 1, 'no needsUpdate');
  assert.equal(batch.geometry.drawRange.count, 40 * 6);
  // One quad moves: only its vertices go up, in every attribute.
  quads[20] = [20 * 20, 5];
  frame(batch, quads);
  assert.equal(batch.uploads, 1);
  assert.deepEqual(ranges(batch), [[20 * 12, 12]]);
  assert.deepEqual(ranges(batch, 'tint'), [[20 * 16, 16]]);
  assert.equal(batch.geometry.getAttribute('position').version, 2);
  // Two far-apart quads: one run spanning them (one upload per attribute).
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  quads[2] = [40, 7]; quads[35] = [700, 7];
  frame(batch, quads);
  assert.equal(batch.uploads, 1);
  assert.deepEqual(ranges(batch), [[2 * 12, 34 * 12]]);
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  quads[10] = [200, 9]; quads[14] = [280, 9];
  frame(batch, quads);
  assert.equal(batch.uploads, 1);
  assert.deepEqual(ranges(batch), [[10 * 12, 5 * 12]]);
});

test('a frame with fewer quads re-uploads nothing it already holds; more quads upload the new ones', () => {
  const batch = new Batch();
  const quads = Array.from({ length: 10 }, (_, i) => [i * 20, 0]);
  frame(batch, quads);
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  frame(batch, quads.slice(0, 6));
  assert.equal(batch.uploads, 0);
  assert.equal(batch.geometry.drawRange.count, 36);
  frame(batch, quads);
  assert.equal(batch.uploads, 0, 'the tail is still what the buffer holds');
  assert.equal(batch.geometry.drawRange.count, 60);
  frame(batch, [...quads, [500, 0]]);
  assert.equal(batch.uploads, 1);
  assert.deepEqual(ranges(batch), [[10 * 12, 12]]);
});

test('a tint change alone is a change, as is a corner uv', () => {
  const batch = new Batch();
  batch.begin(); batch.quad(M, 0, 0, 1, 1, texture, [0, 0, 1, 1], WHITE); batch.end();
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  batch.begin(); batch.quad(M, 0, 0, 1, 1, texture, [0, 0, 1, 1], [1, 1, 1, 0.5]); batch.end();
  assert.equal(batch.uploads, 1);
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  batch.begin(); batch.quadCorners(M, 0, 0, 1, 1, texture, [[0, 0], [1, 0], [0, 1], [1, 1]], [1, 1, 1, 0.5]); batch.end();
  assert.equal(batch.uploads, 1, 'the corner uvs differ from the plain quad');
  for (const attribute of Object.values(batch.geometry.attributes)) attribute.clearUpdateRanges();
  batch.begin(); batch.quadCorners(M, 0, 0, 1, 1, texture, [[0, 0], [1, 0], [0, 1], [1, 1]], [1, 1, 1, 0.5]); batch.end();
  assert.equal(batch.uploads, 0);
});

test('warm batches retain group, range and descriptor storage', () => {
  const batch = new Batch(), quads = [[0,0],[20,0]];
  frame(batch,quads);
  const groups = batch.geometry.groups, group = groups[0], material = batch.mesh.material[0];
  const span = batch.geometry.attributes.position.updateRanges[0];
  quads[0][0] = 4;
  frame(batch,quads);
  assert.equal(batch.geometry.groups,groups);
  assert.equal(groups[0],group);
  assert.equal(batch.mesh.material[0],material);
  assert.equal(material.isMaterial,undefined,'quad bindings do not construct Three materials');
  assert.equal(batch.geometry.attributes.position.updateRanges[0],span);
});
