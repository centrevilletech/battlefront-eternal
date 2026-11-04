// Authenticated jet trooper and droideka replay on installed map collision.
// Exact class, skeleton, animation and geometry bytes are hashed once and
// used directly; an optional native corpus stays outside repositories.
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { selectMission } from '../src/mission.js';
import { loadClips } from '../src/anim.js';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createActorPredictionSim } from '../src/runtime/actor-prediction-sim.js';
import { actorCourse, createPredictionActor, runActorPredictionCase } from '../tests/fixtures/prediction-actor-cases.js';
import { actorCollisionGeometry, actorTraceRecord } from './prediction-actors-collision.mjs';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'source revision required: --revision or BFE_SOURCE_REVISION');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex'), loaded = new Map(), files = new Map();
const read = async path => {
  if (loaded.has(path)) return loaded.get(path);
  const response = await fetch(path); assert.ok(response.ok, `missing content ${path}`);
  const bytes = await response.arrayBuffer(); loaded.set(path, bytes); files.set(path, [path, bytes.byteLength, sha256(new Uint8Array(bytes))]); return bytes;
};
const query = '?map=nab2&side=rep';
const manifest = await contentManifest(query, async path => {
  const response = await fetch(path);
  if (!response.ok) return response;
  const bytes = await response.arrayBuffer();
  if (/\/(map|units|anims)\.json$|\/anims\.bin$|\/(terrain_)?collision(_index|_poly)?\.bin$/.test(path)) {
    loaded.set(path, bytes); files.set(path, [path, bytes.byteLength, sha256(new Uint8Array(bytes))]);
  }
  return { ok: true, status: 200, arrayBuffer: async () => bytes };
});
const parse = bytes => JSON.parse(new TextDecoder().decode(bytes));
const world = selectMission(parse(await read('/maps/nab2/map.json')), new URLSearchParams(query));
const rep = parse(await read('/sides/rep/units.json')), cis = parse(await read('/sides/cis/units.json'));
const jet = Object.entries(rep.units).find(([, d]) => d.base === 'soldier' && d.JetFuelRechargeRate > 0);
const droideka = Object.entries(cis.units).find(([, d]) => d.base === 'walkerdroid');
assert.ok(jet && droideka, 'authenticated jet trooper and droideka definitions');
const common = loadClips(parse(await read('/common/anims.json')), await read('/common/anims.bin'));
const geometry = { positions: new Float32Array(await read('/maps/nab2/collision.bin')), indices: new Uint32Array(await read('/maps/nab2/collision_index.bin')),
  polygons: new Uint32Array(await read('/maps/nab2/collision_poly.bin')), terrainPositions: new Float32Array(await read('/maps/nab2/terrain_collision.bin')),
  terrainIndices: new Uint32Array(await read('/maps/nab2/terrain_collision_index.bin')) };
const spawnPoints = Object.values(world.spawns || {}).flat().filter(p => Array.isArray(p) && p.length >= 4 && p.slice(0,4).every(Number.isFinite));
assert.ok(spawnPoints.length > 1, 'authored actor spawn positions');
function skeleton(meta) {
  const root = new THREE.Group(), bones = meta.bones.map(desc => {
    const bone = new THREE.Bone(); bone.name = desc.name;
    new THREE.Matrix4().fromArray(desc.local).decompose(bone.position, bone.quaternion, bone.scale); return bone;
  });
  for (let i = 0; i < bones.length; i++) (meta.bones[i].parent < 0 ? root : bones[meta.bones[i].parent]).add(bones[i]);
  root.updateMatrixWorld(true); return { root, bones };
}
const model = parse(await read(`/sides/cis/actors/${droideka[1].model}.json`));
const clipMeta = parse(await read(`/sides/cis/actors/${droideka[1].clips}.json`)), clipBuffer = await read(`/sides/cis/actors/${droideka[1].clips}.bin`);
const content = { soldier: { definition: jet[1] }, droideka: { definition: droideka[1], weapons: droideka[1].weapons,
  clips: loadClips(clipMeta, clipBuffer, model.bones.map(b => b.name)), actor: () => skeleton(model) } };
for (const value of Object.values(content)) Object.assign(value, { getupTime: common.get('human_rifle_prone_getup_stand')?.duration ?? 0,
  diveTime: .75 * (common.get('human_rifle_diveforward')?.duration ?? 0), maxFlyHeight: world.mission?.maxFlyHeight ?? 200 });
const scenarios = [
  ...[0, 1].map(i => ({ name: `content-jet-${i}`, kind: 'soldier', ticks: 480, pattern: 'jet', spawn: spawnPoints[i], rollback: true, crowd: true })),
  { name: 'content-droideka-fold-and-roll', kind: 'droideka', ticks: 360, pattern: 'droideka', spawn: spawnPoints[0], rollback: true },
  { name: 'content-droideka-reverse', kind: 'droideka', ticks: 360, pattern: 'reverse', spawn: spawnPoints[1], rollback: true },
  { name: 'content-droideka-quiet-death', kind: 'droideka', ticks: 180, pattern: 'idle', spawn: spawnPoints[0], patch: { dead: true }, rollback: true },
];
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
const instance = await loadSim(), rays = await loadRayKernel(instance);
let corpusBytes = 0, operations = 0, ticks = 0, jetTicks = 0, reverseTicks = 0, completed = false, report;
const emit = event => { const line = JSON.stringify(event) + '\n'; corpusHash.update(line); corpusBytes += Buffer.byteLength(line); if (descriptor !== null) writeSync(descriptor, line); };
try {
  for (const scenario of scenarios) {
    let actual, expected;
    const options = { makeCourse: rays => actorCourse(rays, geometry), create: (s, course) => createPredictionActor(s, course, content[s.kind]) };
    try {
      actual = runActorPredictionCase(scenario, { ...options, rays, makeNative(world) {
        emit({ type: 'collision', label: scenario.name, geometry: actorCollisionGeometry(world) });
        return createActorPredictionSim(instance, world, { trace(event) { emit(actorTraceRecord(event, world, scenario.name)); operations++; } });
      }, after({ prediction }) { assert.ok(prediction.nativeSession.handle && prediction.nativeSession.steps > 0); } });
      expected = runActorPredictionCase(scenario, options);
      assert.deepEqual(actual.observations, expected.observations, scenario.name);
      for (const observation of actual.observations) { stateHash.update(JSON.stringify(observation)); ticks++;
        if (observation.state.air === 'jet') jetTicks++; if (observation.state.reverse) reverseTicks++; }
    } finally { actual?.dispose(); expected?.dispose(); }
  }
  assert.ok(jetTicks > 20 && reverseTicks > 20 && operations > 1000);
  report = { revision, query, contentHash: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)), manifest, files: [...files.values()],
    actors: 2, cases: scenarios.length, ticks, jetTicks, reverseTicks, operations, stateSha256: stateHash.digest('hex'),
    corpusBytes, corpusSha256: corpusHash.digest('hex') }; completed = true;
} finally { if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
