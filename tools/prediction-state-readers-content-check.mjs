// Decode checkpoints from authenticated infantry and recon definitions, then
// restore private copies and continue replay. Native transcripts stay external.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { selectMission } from '../src/mission.js';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { captureArsenalWeapon, validateArsenalWeapon, applyArsenalWeapon } from '../src/runtime/network-arsenal-state.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
import { captureRemoteReplay, restoreRemoteReplay } from '../src/runtime/network-remote-replay.js';
import { encodeRemoteCorrection, decodeRemoteCorrection } from '../src/runtime/network-remote-codec.js';
import { CommandControls } from '../src/runtime/commands.js';
import { actorCourse } from '../tests/fixtures/prediction-actor-cases.js';
import { createWeaponUnit, runWeaponPredictionCase } from '../tests/fixtures/prediction-weapon-cases.js';
import { runRemotePredictionCase, remoteObservation, remoteCommands } from '../tests/fixtures/prediction-remote-cases.js';

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
const definitions = new Map(), loadouts = [];
for (const side of ['rep', 'cis', 'all', 'imp']) {
  const path = `/sides/${side}/units.json`;
  assert.ok(authentication.some(a => a.manifest.some(([name, hash]) => name === path && hash !== 'missing')), `authenticated ${side}`);
  const units = parse(await read(path));
  for (const [name, unit] of Object.entries(units.units)) if (unit.weapons?.length) loadouts.push({ name: `${side}-${name}`, definitions: unit.weapons });
  const walk = value => {
    if (!value || typeof value !== 'object') return;
    if (value.base === 'remote' && value.droid && typeof value.droid === 'object') {
      const digest = sha256(JSON.stringify(value.droid));
      if (!definitions.has(digest)) definitions.set(digest, value.droid);
    }
    for (const child of Object.values(value)) walk(child);
  };
  walk(units);
}
assert.ok(definitions.size > 0 && loadouts.length > 0, 'authenticated definitions');

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
const corpusHash = createHash('sha256'), stateHash = createHash('sha256'), opcodes = {}, bases = new Set();
let corpusBytes = 0, operations = 0, label = '', weaponCheckpoints = 0, meleeCheckpoints = 0, reconCheckpoints = 0, salvoCheckpoints = 0, continuationTicks = 0;
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
const trace = event => {
  const line = JSON.stringify({ label, op: event.op, input: words(event.input), output: words(event.output), error: event.error ?? null }) + '\n';
  corpusHash.update(line); corpusBytes += Buffer.byteLength(line); operations++;
  opcodes[event.op] = (opcodes[event.op] || 0) + 1;
  if (descriptor !== null) writeSync(descriptor, line);
};
function framed(payload, index) {
  const offset = index % 2 ? 1 : 7;
  const bytes = index % 3 ? new Uint8Array(payload.length + offset + 5) : Buffer.alloc(payload.length + offset + 5);
  bytes.set(payload, offset); return bytes.subarray(offset, offset + payload.length);
}
function scenario(loadout) {
  let tick = 0;
  const operations = [{ op: 'begin', tick: 0, epoch: 2 }];
  for (const channel of [0, 1]) {
    let index = 0;
    for (const definition of loadout.definitions.filter(w => w.channel === channel)) {
      if (index++) {
        tick++; operations.push({ op: 'step', tick, command: { tick, sequence: tick, generation: 3, down: [], look: [], actions: [channel ? 'nextSecondary' : 'nextPrimary'] } });
      }
      const held = Math.min(360, Math.max(24, Math.ceil((definition.MaxPressedTime || 0) * 60) + 2));
      for (let i = 0; i < held + 90; i++) {
        tick++;
        operations.push({ op: 'step', tick, presses: [tick * 97, 0x80000000 + tick], command: { tick, sequence: tick, generation: 3, look: [],
          down: i < held ? [channel ? 'Mouse2' : 'Mouse0'] : [], actions: i === 0 ? [channel ? 'fireSecondary' : 'firePrimary'] : i === held + 3 ? ['reload'] : [] } });
      }
    }
  }
  operations.push({ op: 'restore' });
  return { ...loadout, operations };
}
function continueRemote(fixture, count = 12) {
  const { context, droid } = fixture.result, controls = new CommandControls();
  fixture.events.length = 0; fixture.shots.length = 0;
  for (const command of remoteCommands(count, { from: fixture.result.tick, pattern: 'drive' })) {
    controls.accept({ ...command, epoch: 1, sequence: command.tick });
    context.firstPerson = command.firstPerson; context.aimCamera.aspect = command.aspect;
    const scale = Math.min(command.mouseScale, 2);
    for (const [dx, dy] of command.look) if (context.active) context.look(dx, dy, scale, command.invertMouse);
    if (context.active) context.updateCamera(1 / 60, Boolean(context.firstPerson), true);
    context.update(1 / 60, { forward: controls.axis('forward'), right: controls.axis('right'), turn: controls.axis('turn') }, controls);
  }
  return remoteObservation(context, droid);
}
const native = createCorrectionSim(await loadSim(), { trace });
let completed = false, report;
try {
  for (const loadout of loadouts) {
    const restored = createWeaponUnit(loadout.definitions).combat.arsenal;
    let replay;
    try {
      replay = runWeaponPredictionCase(scenario(loadout), { after({ unit, operation }) {
        if (operation.op !== 'step' || operation.tick % 6) return;
        for (const [i, weapon] of unit.combat.arsenal.weapons.entries()) {
          label = `${loadout.name}-${operation.tick}-${i}`;
          const bytes = framed(captureArsenalWeapon(weapon), weaponCheckpoints), before = Uint8Array.from(bytes), first = native.calls;
          const expected = validateArsenalWeapon(weapon, bytes), actual = validateArsenalWeapon(weapon, bytes, native);
          assert.ok(native.calls - first > 1, `${label} native delta delegation`);
          assert.equal(actual.weapon, weapon); assert.deepEqual(actual.state, expected.state); assert.deepEqual(actual.melee, expected.melee);
          assert.equal(JSON.stringify(actual.state), JSON.stringify(expected.state), `${label} property order`);
          const target = restored.weapons[i], levels = target.levels, random = target.random;
          applyArsenalWeapon({ ...actual, weapon: target });
          assert.equal(target.levels, levels); assert.equal(target.random, random);
          assert.equal(target.def, target.levels[actual.state.level]); assert.equal(target.level, actual.state.level);
          assert.deepEqual(captureWeaponState(target), captureWeaponState(weapon), `${label} restored weapon bytes`);
          assert.deepEqual(target.melee?.captureState(), weapon.melee?.captureState(), `${label} restored melee`);
          assert.deepEqual(new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength), before);
          stateHash.update(bytes); bases.add(weapon.def.base); weaponCheckpoints++; if (actual.melee) meleeCheckpoints++;
        }
      } });
    } finally { replay?.prediction.restore(); replay?.prediction.dispose?.(); }
  }
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
    const actualWorld = actorCourse(null, geometry), expectedWorld = actorCourse(null, geometry);
    try {
      let defIndex = 0;
      for (const definition of definitions.values()) {
        for (let spawn = 0; spawn < 3; spawn++) for (const pattern of ['drive', 'destruct']) for (const ticks of [1, 7, 30, 60, 120]) {
          const position = positions[spawn].slice(0, 3); position[1] += 2;
          const destruct = definition.weapons?.find(w => w.base === 'destruct');
          const test = { name: `recon-${map}-${defIndex}-${spawn}-${pattern}-${ticks}`, ticks, pattern, exactDefinition: true,
            definition, position, yaw: positions[spawn][3], velocity: [1.25, -.5, .75],
            ...(pattern === 'destruct' && destruct ? { patch: { countdown: Math.max(0, (destruct.MaxPressedTime ?? 0) - .25) } } : {}) };
          label = test.name;
          const actual = runRemotePredictionCase(test, { collision: actualWorld.world }), expected = runRemotePredictionCase(test, { collision: expectedWorld.world });
          assert.deepEqual(actual.observation, expected.observation, `${label} initial replay`);
          const actualDroid = actual.result.droid, expectedDroid = expected.result.droid;
          const payload = encodeRemoteCorrection(captureRemoteReplay(actual.result.context, actualDroid), actualDroid);
          assert.deepEqual(payload, encodeRemoteCorrection(captureRemoteReplay(expected.result.context, expectedDroid), expectedDroid));
          const bytes = framed(payload, reconCheckpoints), before = Uint8Array.from(bytes), first = native.calls;
          const actualState = decodeRemoteCorrection(bytes, actualDroid, native), expectedState = decodeRemoteCorrection(bytes, expectedDroid);
          assert.ok(native.calls - first > 1, `${label} native recon delegation`);
          assert.deepEqual(actualState, expectedState, `${label} recon state`);
          assert.equal(JSON.stringify(actualState), JSON.stringify(expectedState), `${label} property order`);
          assert.deepEqual(new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength), before);
          if (actualState.salvo) {
            assert.ok(actualDroid.arsenal.weapons.some(w => w.levels.includes(actualState.salvo.def)), `${label} salvo definition identity`);
            salvoCheckpoints++;
          }
          const vectors = ['position', 'velocity', 'eye', 'aimDir', 'target'].map(key => [key, actualDroid[key]]);
          const omitted = actualDroid.arsenal.weapons.map(w => [w.salvoRandom, w.salvoSeed, w.salvoCount, w.random]);
          for (const droid of [actualDroid, expectedDroid]) for (const weapon of droid.arsenal.weapons) weapon.level = 791;
          restoreRemoteReplay(actual.result.context, actualDroid, actualState);
          restoreRemoteReplay(expected.result.context, expectedDroid, expectedState);
          for (const [key, value] of vectors) assert.equal(actualDroid[key], value, `${label} ${key} alias`);
          actualDroid.arsenal.weapons.forEach((w, i) => {
            assert.equal(w.level, 791, `${label} selective numeric level`);
            assert.deepEqual([w.salvoRandom, w.salvoSeed, w.salvoCount, w.random], omitted[i], `${label} omitted cursor fields`);
          });
          const continued = continueRemote(actual), baseline = continueRemote(expected);
          assert.deepEqual(continued, baseline, `${label} continued state`);
          assert.deepEqual(actual.events, expected.events, `${label} callback order`);
          assert.deepEqual(actual.shots, expected.shots, `${label} shot descriptors`);
          stateHash.update(bytes); stateHash.update(JSON.stringify(continued)); reconCheckpoints++; continuationTicks += 12;
        }
        defIndex++;
      }
    } finally { actualWorld.dispose(); expectedWorld.dispose(); }
  }
  assert.ok(weaponCheckpoints > 500 && reconCheckpoints >= 60 && salvoCheckpoints > 0);
  assert.ok(Object.keys(opcodes).filter(op => Number(op) > 8).length > 5, 'staged native paths covered');
  report = { revision, authentication, files: [...files.values()], definitionHashes: [...definitions.keys()],
    loadouts: loadouts.length, reconDefinitions: definitions.size, maps: 2, weaponCheckpoints, meleeCheckpoints, reconCheckpoints, salvoCheckpoints,
    continuationTicks, bases: [...bases].sort(), operations, opcodes, stateSha256: stateHash.digest('hex'), corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { native.dispose(); if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
