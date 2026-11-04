// Private hull replay with authenticated vehicle classes, geometry and walk
// clips. The content manifest and full file hashes identify the exact inputs.
// An optional owned trace stays outside repositories for native replay.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { loadSim } from '../src/sim/module.js';
import { createVehiclePredictionSim } from '../src/runtime/vehicle-prediction-sim.js';
import { hullFromPack, runHullPredictionCase } from '../tests/fixtures/prediction-hull-cases.js';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'source revision required: --revision or BFE_SOURCE_REVISION');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex'), loaded = new Map(), authenticated = [];
const fingerprintFetch = async path => {
  const response = await fetch(path);
  if (!response.ok || !/\/vehicles\/([^/]+\/)?vehicles\.json$/.test(path)) return response;
  const bytes = await response.arrayBuffer(); loaded.set(path, bytes);
  return { ok: true, status: 200, arrayBuffer: async () => bytes };
};
const packs = [];
for (const query of ['?map=nab2&side=rep', '?map=geo1&side=rep', '?map=hot1&side=all', '?map=bes1&side=all']) {
  const manifest = await contentManifest(query, fingerprintFetch);
  const path = manifest.find(([name, hash]) => /\/vehicles\/([^/]+\/)?vehicles\.json$/.test(name) && hash !== 'missing')?.[0];
  assert.ok(path && loaded.has(path), 'authenticated vehicle pack');
  const bytes = loaded.get(path), data = JSON.parse(new TextDecoder().decode(bytes)), files = [[path, bytes.byteLength, sha256(new Uint8Array(bytes))]];
  const buffers = {};
  for (const [key, file] of [['buffer', 'models.bin'], ['indexBuffer', 'models_index.bin'], ['animationBuffer', 'animations.bin']]) {
    const name = path.slice(0, path.lastIndexOf('/') + 1) + file, response = await fetch(name);
    if (response.status === 404 && key === 'animationBuffer' && !Object.keys(data.animations || {}).length) { buffers[key] = new ArrayBuffer(0); files.push([name, 0, 'missing']); continue; }
    assert.ok(response.ok, `missing vehicle buffer ${file}`);
    buffers[key] = await response.arrayBuffer(); files.push([name, buffers[key].byteLength, sha256(new Uint8Array(buffers[key]))]);
  }
  authenticated.push({ query, hash: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)), manifest, files });
  packs.push({ data, buffers });
}

const corpusPath = option('corpus') ? resolve(option('corpus')) : null;
if (corpusPath) {
  const within = relative(root, corpusPath);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'content corpus must stay outside the checkout');
  for (let dir = dirname(corpusPath); ; dir = dirname(dir)) {
    assert.equal(existsSync(join(dir, '.git')), false, 'content corpus must stay outside repositories');
    if (dir === dirname(dir)) break;
  }
}
const descriptor = corpusPath ? openSync(corpusPath, 'wx') : null, corpusHash = createHash('sha256'), stateHash = createHash('sha256');
const words = values => { const bytes = Buffer.alloc(values.length * 8); for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8); return bytes.toString('hex'); };
const kinds = {}, unique = new Set();
let label = '', hulls = 0, observations = 0, operations = 0, queries = 0, corpusBytes = 0, completed = false, report, native;
try {
  native = createVehiclePredictionSim(await loadSim(), { trace(event) {
    const line = JSON.stringify({ label, session: String(event.session), op: event.op, input: words(event.input), output: words(event.output), error: event.error ?? null,
      queries: (event.queries || []).map(q => ({ op: q.op, args: words(q.args), values: words(q.values) })) }) + '\n';
    corpusHash.update(line); corpusBytes += Buffer.byteLength(line); if (descriptor !== null) writeSync(descriptor, line); operations++;
  } });
  for (const { data, buffers } of packs) for (const [name, def] of Object.entries(data.classes)) {
    if (!['hover', 'walker', 'commandwalker', 'flyer'].includes(def.base || 'hover')) continue;
    const id = sha256(JSON.stringify([name, def, data.models[def.model], data.animations?.[def.model]]));
    if (unique.has(id)) continue; unique.add(id);
    const kind = def.base === 'flyer' ? 'flyer' : (def.settings?.HealthType || '').toLowerCase() === 'animal' ? 'creature'
      : ['walker', 'commandwalker'].includes(def.base) ? 'walker' : /hailfire/i.test(name) ? 'wheeled' : def.springs?.length ? 'springs' : 'hover';
    kinds[kind] = (kinds[kind] || 0) + 1; label = `content-hull-${hulls++}`;
    const pack = { ...data, classes: { [name]: def }, models: { [def.model]: data.models[def.model] },
      collision: { [def.model]: data.collision[def.model] }, animations: data.animations?.[def.model] ? { [def.model]: data.animations[def.model] } : {} };
    const create = () => hullFromPack(pack, buffers, name), scenario = { name: label, kind, ticks: 360, batch: 20, crew: true };
    let actual, expected;
    try {
      actual = runHullPredictionCase(scenario, { native, create }); expected = runHullPredictionCase(scenario, { create });
      assert.deepEqual(actual.observations, expected.observations, label);
      assert.deepEqual(actual.events, expected.events, `${label} callback order`);
      for (const o of actual.observations) { stateHash.update(JSON.stringify(o)); observations++; }
      queries += actual.events.length;
      if (kind === 'walker' || kind === 'creature') assert.ok(actual.prediction.walkBlend > 0 || !actual.prediction.pose, `${label} walks after physics`);
    } finally { actual?.dispose(); expected?.dispose(); }
  }
  assert.ok(kinds.flyer > 0 && kinds.walker > 0 && (kinds.hover || kinds.springs || kinds.wheeled), 'flight, walker and ground classes exercised');
  assert.ok(hulls >= 8); assert.ok(operations > 1000); assert.ok(queries > 1000);
  report = { revision, content: authenticated, hulls, kinds, ticksPerHull: 360, observations, queries, operations,
    stateSha256: stateHash.digest('hex'), corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { native?.dispose(); if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
