import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { DebrisPieces, heatMaterial, pieceShape } from '../src/destruction-debris.js';
import { DstPiece as PIECE } from '../src/environment/layout.js';
import { zeroMaterial } from '../src/materials.js';

test('a piece shape is its box centre, half size and extreme vertices', () => {
  const group = new THREE.Group();
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.2, 0.4));
  mesh.position.set(0.3, 0, 0);
  group.add(mesh);
  const shape = pieceShape(group);
  assert.ok(shape.center.distanceTo(new THREE.Vector3(0.3, 0, 0)) < 1e-6);
  assert.ok(shape.half.distanceTo(new THREE.Vector3(0.3, 0.1, 0.2)) < 1e-6);
  assert.equal(shape.points.length, 8, 'a box touches with its corners');
  for (const p of shape.points) assert.ok(Math.abs(Math.abs(p.x) - 0.3) < 1e-6 && Math.abs(Math.abs(p.y) - 0.1) < 1e-6);
  assert.ok(Math.abs(shape.radius - Math.hypot(0.3, 0.1, 0.2)) < 1e-6);
  // A 1 kg box: I = (b^2 + c^2) / 3 for half sizes b, c.
  assert.ok(Math.abs(1 / shape.inverseInertia.x - (0.01 + 0.04) / 3) < 1e-6);
});

// The pieces' bodies are the Rust phase's; the shell keeps their models.
function model() {
  const material = zeroMaterial('dynamic', {});
  return () => {
    const group = new THREE.Group();
    group.add(new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.15, 0.3), material));
    return group;
  };
}

test('heated pieces keep the stock material and add heat, charring and crumbling', () => {
  const source = zeroMaterial('dynamic', {});
  const heated = heatMaterial(source);
  assert.notEqual(heated, source);
  assert.equal(heatMaterial(source), heated, 'made once per material');
  assert.ok(!source.fragmentShader.includes('heated('), 'the stock material is unchanged');
  assert.ok(heated.fragmentShader.includes('color.rgb = heated(color.rgb);'));
  assert.ok(heated.vertexShader.includes('vHeatLocal = transformed;'));
  assert.equal(heated.defines.DYNAMIC, '');
  assert.equal(heated.defines.HEATED, '');
  // The map's light uniforms are shared, not copied.
  assert.equal(heated.uniforms.sunDirection, source.uniforms.sunDirection);
  assert.notEqual(heated.uniforms.sunScale, source.uniforms.sunScale);
});

test('heat is applied before a piece dissolves into the far scene, so it fades out at range', () => {
  const heated = heatMaterial(zeroMaterial('dynamic', {}));
  const shader = heated.fragmentShader;
  const heat = shader.indexOf('color.rgb = heated(color.rgb);');
  const fade = shader.indexOf('// Linear fog by depth');
  const near = shader.indexOf('float near = clamp((fade.y - vDepth)');
  assert.ok(heat > 0 && heat < fade && heat < near, 'heat, then the fog and the near-to-far dissolve');
});


// A record of one piece, as the phase writes them.
function record(uid, heat = 1) {
  const r = new Float64Array(PIECE.STRIDE);
  r[PIECE.UID] = uid;
  r[PIECE.HEAT] = heat;
  r[PIECE.SOOT] = 1;
  r[PIECE.MOVED] = 1;
  r[PIECE.SHADOWS] = 1;
  r[PIECE.QUATERNION + 3] = 1;
  r[PIECE.POSITION + 1] = 2;
  r.set([0.1, 0.2, 0.3, 0.4], PIECE.TEAR);
  return r;
}

test('the pieces follow the phase\'s records by their ids: made, set, let go, recycled; a recycled model starts its upscaler motion afresh', () => {
  const shapes = [];
  const scene = new THREE.Scene();
  const pieces = new DebrisPieces({ scene, model: (name) => (name === 'chunk' ? model() : null), host: { shape: (handle, shape) => shapes.push([handle, shape.points.length]) } });
  assert.equal(pieces.handle('chunk'), 0);
  assert.deepEqual(shapes, [[0, 8]], 'its shape went to the phase once');
  assert.equal(pieces.handle('chunk'), 0);
  assert.equal(pieces.handle('missing'), -1, 'no model, no shape');
  pieces.sync(record(1), 0, 1);
  const [first] = pieces.pieces;
  assert.ok(first.object.visible && first.object.parent === scene);
  assert.equal(first.object.userData.heated.heat, 1);
  assert.deepEqual(first.object.userData.heated.tear.toArray(), [0.1, 0.2, 0.3, 0.4], 'hot where it tore away');
  assert.equal(first.object.position.y, 2, 'placed from its record');
  const proxy = first.object.children[0].userData.motion;
  assert.ok(proxy, 'its meshes are tracked for the upscaler');
  proxy.userData.seen = 41; // drawn last frame
  // The phase lists a second piece and not the first: the first goes, the second takes its model.
  pieces.sync(record(2), 0, 1);
  assert.deepEqual(pieces.pieces.map((p) => p.uid), [2], 'paired by id, not place');
  assert.equal(pieces.pieces[0].object, first.object, 'the same model, recycled');
  assert.equal(proxy.userData.seen, -1, 'no motion carried over from where it was');
  // A record the page has no model for is skipped, and an empty list lets everything go.
  const odd = record(3);
  odd[PIECE.SHAPE] = 9;
  pieces.sync(odd, 0, 1);
  assert.deepEqual(pieces.pieces.map((p) => p.uid), [], 'a piece of an unknown model is not shown');
  pieces.sync(record(4), 0, 1);
  pieces.sync(record(4), 0, 0);
  assert.equal(pieces.count, 0);
});
