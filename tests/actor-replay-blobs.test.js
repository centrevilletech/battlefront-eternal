import test from 'node:test';
import assert from 'node:assert/strict';
import { replayBlobStore } from '../tools/renderer-corpus/replay-blobs.mjs';

test('replay snapshots share identical immutable bytes across offset views', () => {
  const target = { blobs: {} }, store = replayBlobStore(target);
  const first = store(Uint8Array.of(1, 2, 3));
  assert.equal(store(Uint8Array.of(9, 1, 2, 3, 9).subarray(1, 4)), first);
  assert.equal(Object.keys(target.blobs).length, 1);
  assert.deepEqual([...target.blobs[first]], [1, 2, 3]);
  target.blobs = {};
  const next = store(Uint8Array.of(4, 5, 6));
  assert.deepEqual([...target.blobs[next]], [4, 5, 6]);
  assert.notEqual(store(Uint8Array.of(1, 2, 3)), next);
});

test('replay storage preserves typed layouts and floating-point bit patterns', () => {
  const target = { blobs: {} }, store = replayBlobStore(target);
  const a = new Uint32Array([0x7fc00001]), b = new Uint32Array([0x7fc00002]);
  const nan = store(new Float32Array(a.buffer));
  assert.equal(store(new Float32Array(a.slice().buffer)), nan);
  assert.notEqual(store(new Float32Array(b.buffer)), nan);
  assert.notEqual(store(a), nan);
  assert.notEqual(store(Float32Array.of(-0)), store(Float32Array.of(0)));
});

test('a texture-buffer hash collision still keeps distinct snapshots', () => {
  const target = { blobs: {} }, store = replayBlobStore(target);
  // Equal-length buffers with the same FNV-1a hash, 3087366838.
  const a = Uint8Array.of(142, 79, 196, 176, 23, 41, 83, 29);
  const b = Uint8Array.of(103, 145, 139, 135, 175, 45, 22, 201);
  const ka = store(a), kb = store(b);
  assert.notEqual(ka, kb);
  assert.equal(store(a.slice()), ka);
  assert.equal(store(b.slice()), kb);
});
