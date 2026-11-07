import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { uploadReplayBlobs } from '../tools/renderer-corpus/blob-transfer.mjs';

test('chunked CDP upload preserves typed bytes without embedding payload in source', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'actor-blobs-'));
  try {
    for(const compression of [undefined,'gzip']){
    const words = Uint32Array.of(0x7fc00001, 0x80000000, 0x3f800000);
    const file = join(dir, 'pixels.bin');
    await writeFile(file, compression ? gzipSync(new Uint8Array(words.buffer)) : new Uint8Array(words.buffer));
    const table = {}, declarations = [], chunks = [];
    let released = false;
    const connection = { async psend(method, args) {
      if (method === 'Runtime.evaluate') return { result: { objectId: 'blobs' } };
      if (method === 'Runtime.releaseObject') { released = true; return {}; }
      assert.equal(args.objectId, 'blobs');
      declarations.push(args.functionDeclaration);
      const values = args.arguments.map(a => a.value);
      if (values.length === 3 && typeof values[1] === 'number') chunks.push(Buffer.from(values[2], 'base64').length);
      const fn = new Function('window', `return (${args.functionDeclaration});`)(globalThis);
      return { result: { value: fn.apply(table, values) } };
    } };
    await uploadReplayBlobs(connection, { pixels: { type: 'Float32Array', file, compression } }, 5);
    assert.ok(released);
    assert.deepEqual(chunks, [5, 5, 2]);
    assert.equal(table.pixels.constructor, Float32Array);
    assert.deepEqual([...new Uint32Array(table.pixels.buffer)], [...words]);
    assert.ok(declarations.every(source => !source.includes(Buffer.from(words.buffer).toString('base64'))));
    }
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('a failed browser upload releases its remote table handle', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'actor-blobs-'));
  try {
    const file = join(dir, 'pixels.bin'); await writeFile(file, Uint8Array.of(1));
    let released = false;
    const connection = { async psend(method) {
      if (method === 'Runtime.evaluate') return { result: { objectId: 'blobs' } };
      if (method === 'Runtime.releaseObject') { released = true; return {}; }
      return { exceptionDetails: { text: 'allocation rejected' } };
    } };
    await assert.rejects(uploadReplayBlobs(connection, { pixels: { type: 'Uint8Array', file } }), /allocation rejected/);
    assert.ok(released);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
