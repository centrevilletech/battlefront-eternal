// Private recon replay against authenticated definitions and map collision.
// Optional continuation traces stay outside the repository and carry a
// closed source/content checksum manifest for native replay.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { selectMission } from '../src/mission.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createRemotePredictionSim } from '../src/runtime/remote-prediction-sim.js';
import { actorCourse } from '../tests/fixtures/prediction-actor-cases.js';
import { runRemotePredictionCase } from '../tests/fixtures/prediction-remote-cases.js';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'source revision required: --revision or BFE_SOURCE_REVISION');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const files = new Map(), loaded = new Map(), authentication = [];
const read = async path => {
  if (loaded.has(path)) return loaded.get(path);
  const response = await fetch(path); assert.ok(response.ok, `missing content ${path}`);
  const bytes = await response.arrayBuffer(); loaded.set(path, bytes);
  files.set(path, [path, bytes.byteLength, sha256(new Uint8Array(bytes))]); return bytes;
};
const parse = bytes => JSON.parse(new TextDecoder().decode(bytes));
for (const query of ['?map=nab2&side=rep', '?map=hot1&side=all']) {
  const manifest = await contentManifest(query, async path => {
    const response = await fetch(path); if (!response.ok) return response;
    const bytes = await response.arrayBuffer();
    if (/\/(map|units)\.json$|\/(terrain_)?collision(_index|_poly)?\.bin$/.test(path)) {
      loaded.set(path, bytes); files.set(path, [path, bytes.byteLength, sha256(new Uint8Array(bytes))]);
    }
    return { ok: true, status: 200, arrayBuffer: async () => bytes };
  });
  authentication.push({ query, hash: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)), manifest });
}
const definitions = new Map();
for (const side of ['rep', 'cis', 'all', 'imp']) {
  const path = `/sides/${side}/units.json`;
  assert.ok(authentication.some(a => a.manifest.some(([name, hash]) => name === path && hash !== 'missing')), `authenticated ${side}`);
  const walk = value => {
    if (!value || typeof value !== 'object') return;
    if (value.base === 'remote' && value.droid && typeof value.droid === 'object') {
      const digest = sha256(JSON.stringify(value.droid));
      if (!definitions.has(digest)) definitions.set(digest, value.droid);
    }
    for (const child of Object.values(value)) walk(child);
  };
  walk(parse(await read(path)));
}
assert.ok(definitions.size > 0, 'authenticated recon definitions');

const corpusPath = option('corpus') ? resolve(option('corpus')) : null;
if (corpusPath) {
  const within = relative(root, corpusPath);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'content corpus must stay outside the checkout');
  for (let dir = dirname(corpusPath); ; dir = dirname(dir)) {
    assert.equal(existsSync(join(dir, '.git')), false, 'content corpus must stay outside repositories');
    if (dir === dirname(dir)) break;
  }
}
const descriptor = corpusPath ? openSync(corpusPath, 'wx') : null;
const corpusHash = createHash('sha256'), stateHash = createHash('sha256');
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let corpusBytes = 0, operations = 0, steps = 0, cases = 0, shots = 0, queries = 0, completed = false, report;
const emit = event => {
  const line = JSON.stringify(event) + '\n'; corpusHash.update(line); corpusBytes += Buffer.byteLength(line);
  if (descriptor !== null) writeSync(descriptor, line);
};
const instance = await loadSim(), rays = await loadRayKernel(instance);
try {
  for (const map of ['nab2', 'hot1']) {
    const side = map === 'nab2' ? 'rep' : 'all';
    const mission = selectMission(parse(await read(`/maps/${map}/map.json`)), new URLSearchParams(`map=${map}&side=${side}`));
    const positions = Object.values(mission.spawns || {}).flat().filter(p => Array.isArray(p) && p.length >= 4 && p.slice(0, 4).every(Number.isFinite));
    assert.ok(positions.length >= 3, 'authored recon launch positions');
    const geometry = {
      positions: new Float32Array(await read(`/maps/${map}/collision.bin`)), indices: new Uint32Array(await read(`/maps/${map}/collision_index.bin`)),
      polygons: new Uint32Array(await read(`/maps/${map}/collision_poly.bin`)), terrainPositions: new Float32Array(await read(`/maps/${map}/terrain_collision.bin`)),
      terrainIndices: new Uint32Array(await read(`/maps/${map}/terrain_collision_index.bin`)),
    };
    const actualWorld = actorCourse(rays, geometry), expectedWorld = actorCourse(null, geometry);
    try {
      let defIndex = 0;
      for (const definition of definitions.values()) {
        for (let spawn = 0; spawn < 3; spawn++) for (const pattern of ['drive', 'destruct']) {
          const position = positions[spawn].slice(0, 3); position[1] += 2;
          const destruct = definition.weapons?.find(w => w.base === 'destruct');
          const scenario = { name: `content-${map}-${defIndex}-${spawn}-${pattern}`, ticks: 120, pattern, exactDefinition: true,
            definition, position, yaw: positions[spawn][3], velocity: [1.25, -.5, .75],
            ...(pattern === 'destruct' && destruct ? { patch: { countdown: Math.max(0, (destruct.MaxPressedTime ?? 0) - .25) } } : {}) };
          const sessions = new Set();
          const native = createRemotePredictionSim(instance, { trace(event) {
            sessions.add(String(event.session)); operations++;
            emit({ label: scenario.name, session: String(event.session), op: event.op,
              input: words(event.input), output: words(event.output), error: event.error ?? null });
          } });
          try {
            const actual = runRemotePredictionCase(scenario, { native, collision: actualWorld.world });
            const expected = runRemotePredictionCase(scenario, { collision: expectedWorld.world });
            assert.deepEqual(actual.observation, expected.observation, `${scenario.name} state`);
            assert.deepEqual(actual.events, expected.events, `${scenario.name} callback and query order`);
            assert.deepEqual(actual.shots, expected.shots, `${scenario.name} shot descriptors`);
            assert.equal(actual.result.tick, expected.result.tick);
            assert.ok(actual.result.nativeSession.steps > 0, `${scenario.name} native delegation`);
            assert.equal(native.activeSessions, 0);
            stateHash.update(JSON.stringify(actual.observation)); stateHash.update(JSON.stringify(actual.events));
            cases++; steps += actual.result.nativeSession.steps; shots += actual.shots.length; queries += actual.events.length;
          } finally { native.dispose(); }
          for (const session of sessions) emit({ type: 'dispose', label: scenario.name, session });
        }
        defIndex++;
      }
    } finally { actualWorld.dispose(); expectedWorld.dispose(); }
  }
  assert.ok(cases >= 12 && steps >= 1440 && shots > 0 && queries > 5000);
  report = { revision, authentication, files: [...files.values()], definitionHashes: [...definitions.keys()],
    definitions: definitions.size, maps: 2, cases, steps, shots, queries, operations,
    stateSha256: stateHash.digest('hex'), corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
