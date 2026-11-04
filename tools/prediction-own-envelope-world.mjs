// Observe live multiplayer corrections without changing the battle. Use with
// world-hash.mjs and the native simulation; optional transcripts stay external.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { root } from './headless-world.mjs';
import { capturePlayerCorrection } from '../src/runtime/network-entity-capture.js';
import { decodeOwnPlayer } from '../src/runtime/network-entity-player.js';
import { createCorrectionBridge } from '../src/runtime/correction-sim.js';
import { contentManifest } from '../src/runtime/content-hash.js';

const sha256 = value => createHash('sha256').update(value).digest('hex');
const revision = process.env.BFE_SOURCE_REVISION || process.env.BFE_COMMIT;
assert.match(revision || '', /^[a-f0-9]{40}$/, 'live envelope proof requires an explicit source revision');
const corpusDirectory = process.env.BFE_OWN_ENVELOPE_DIR ? realpathSync(resolve(process.env.BFE_OWN_ENVELOPE_DIR)) : null;
if (corpusDirectory) {
  const within = relative(realpathSync(root), corpusDirectory);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'envelope corpus must stay outside the checkout');
  for (let directory = corpusDirectory; ; directory = dirname(directory)) {
    assert.equal(existsSync(join(directory, '.git')), false, 'envelope corpus must stay outside repositories');
    if (directory === dirname(directory)) break;
  }
}
const corpusHash = createHash('sha256'), payloadHash = createHash('sha256');
const modes = {}, schemas = {}, queries = new Set();
let current = null, descriptor = null, corpusPath = null, label = '', samples = 0, calls = 0, bytes = 0, battles = 0;
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
function trace(event) {
  assert.equal(event.op, 8, 'live envelope parser delegated');
  assert.equal(event.error, null, 'published envelope is structurally valid');
  const line = JSON.stringify({ label, op: event.op, input: words(event.input), output: words(event.output), error: null }) + '\n';
  corpusHash.update(line); bytes += Buffer.byteLength(line); calls++;
  if (descriptor !== null) writeSync(descriptor, line);
}

export function battle({ game, server, world, index, options }) {
  assert.equal(current, null);
  assert.ok(game.simBridge, 'live envelope proof requires the shared simulation bridge');
  assert.ok(server.host.registry, 'authenticated live registry');
  if (!battles && corpusDirectory) {
    const key = sha256(JSON.stringify(options)).slice(0, 16);
    corpusPath = join(corpusDirectory, `envelope-${options.map}-${options.side}-${key}.jsonl`);
    assert.equal(existsSync(corpusPath + '.manifest.json'), false);
    descriptor = openSync(corpusPath, 'wx');
  }
  current = { game, server, world, index, native: createCorrectionBridge(game.simBridge, { trace }) };
  queries.add(options.query); battles++;
}

export function ticked(tick) {
  if (tick % 7) return;
  const { game, server, world, index, native } = current;
  const captured = capturePlayerCorrection(game, world, server.id, { registry: server.host.registry });
  const offset = samples % 2 ? 1 : 7;
  const storage = samples % 3 ? new Uint8Array(captured.payload.length + offset + 5) : Buffer.alloc(captured.payload.length + offset + 5);
  storage.set(captured.payload, offset);
  const record = { ...captured, payload: storage.subarray(offset, offset + captured.payload.length) };
  const before = Uint8Array.from(record.payload);
  label = `battle-${index + 1}-tick-${tick}`;
  const first = native.calls, expected = decodeOwnPlayer(record), actual = decodeOwnPlayer(record, native);
  assert.equal(native.calls - first, 1, 'one native envelope read per live correction');
  assert.deepEqual(actual, expected, label);
  assert.equal(JSON.stringify(actual), JSON.stringify(expected), `${label} property and serialization order`);
  assert.deepEqual(new Uint8Array(record.payload.buffer, record.payload.byteOffset, record.payload.byteLength), before, `${label} source bytes unchanged`);
  modes[actual.controlMode] = (modes[actual.controlMode] || 0) + 1;
  schemas[actual.bodySchema] = (schemas[actual.bodySchema] || 0) + 1;
  payloadHash.update(record.payload); samples++;
}

export function ended() {
  current.native.dispose(); current = null;
}

export async function finish() {
  assert.equal(current, null);
  assert.ok(samples > 0 && samples === calls, 'live correction samples reached native decoding');
  if (descriptor !== null) { closeSync(descriptor); descriptor = null; }
  const authentication = [];
  for (const query of queries) {
    const files = [];
    const manifest = await contentManifest(query, async path => {
      const response = await fetch(path);
      if (!response.ok) return response;
      const bytes = await response.arrayBuffer();
      files.push([path, bytes.byteLength, sha256(new Uint8Array(bytes))]);
      return { ok: true, status: 200, arrayBuffer: async () => bytes };
    });
    authentication.push({ query, manifest, files, fingerprint: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)) });
  }
  const report = { closed: true, revision, battles, samples, calls, modes, schemas, authentication,
    payloadSha256: payloadHash.digest('hex'), corpusBytes: bytes, corpusSha256: corpusHash.digest('hex') };
  if (corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  console.error(JSON.stringify({ envelopeProof: { revision, battles, samples, calls, modes, schemas, corpusBytes: bytes } }));
}
