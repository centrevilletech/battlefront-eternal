// Usage: node tools/benchmark-entity-input.mjs /absolute/path/to/baseline.mjs
// Export the original standalone input codec with git show before running.
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import * as direct from '../src/runtime/network-entity-input.js';
if (!process.argv[2]) throw Error('Pass the baseline input codec module path');
const baseline = await import(pathToFileURL(resolve(process.argv[2])));
let seed = 12345;
const random = () => ((seed = Math.imul(seed, 1664525) + 1013904223 >>> 0) / 4294967296);
const packets = [];
for (let i = 0; i < 100; i++) {
  const packet = {epoch: i + 1, generation: 2,
    commands: Array.from({length: i % 8 + 1}, (_, n) => ({sequence: n + 1, tick: n + 6,
      down: ['KeyW', 'Mouse0'], lookTotal: [random() * 200 - 100, random() * 200 - 100],
      mouseScale: 1, aspect: 16 / 9,
      ...(i % 2 ? {gamepad: {axes: [random(), random(), random(), random()], buttons: [{pressed: true}]},
        flightSettings: {sensitivity: 1, curve: 1.35, bindings: {}}} : {})})), receipts: [i + 1]};
  const bytes = baseline.encodeEntityInput(packet);
  assert.deepEqual(direct.encodeEntityInput(packet), bytes);
  assert.deepEqual(direct.decodeEntityInput(bytes), baseline.decodeEntityInput(bytes));
  packets.push(bytes);
}
const read = (codec, bytes) => {
  try { return {ok: true, value: codec.decodeEntityInput(bytes)}; }
  catch { return {ok: false}; }
};
for (let i = 0; i < 1000; i++) {
  const bytes = packets[i % packets.length].slice();
  bytes[Math.floor(random() * bytes.length)] = Math.floor(random() * 256);
  assert.deepEqual(read(direct, bytes), read(baseline, bytes));
}
if (process.argv.includes('--check-only')) {
  console.log(JSON.stringify({node: process.version, differential: {validPackets: 100, byteMutations: 1000}}));
  process.exit(0);
}
for (let i = 0; i < 2000; i++) {
  baseline.decodeEntityInput(packets[i % 100]);
  direct.decodeEntityInput(packets[i % 100]);
}
const runs = [];
for (let pass = 0; pass < 4; pass++) {
  const order = pass % 2 ? [['direct', direct], ['baseline', baseline]] : [['baseline', baseline], ['direct', direct]];
  for (const [name, codec] of order) {
    const start = performance.now();
    for (let i = 0; i < 5000; i++) codec.decodeEntityInput(packets[i % 100]);
    runs.push({name, packets: 5000, ms: performance.now() - start});
  }
}
console.log(JSON.stringify({node: process.version, seed: 12345,
  differential: {validPackets: 100, byteMutations: 1000},
  fixture: 'Alternating keyboard/controller packets, 1–8 commands, one receipt', runs}));
