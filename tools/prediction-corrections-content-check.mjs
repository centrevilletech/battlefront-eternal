// Restore authenticated hull checkpoints, then continue private simulation on
// both readers. Optional native transcripts carry source/content checksums and
// stay outside repositories.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join, basename } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { CommandControls } from '../src/runtime/commands.js';
import { networkVehicleInput } from '../src/runtime/network-vehicle.js';
import { predictionVehicle } from '../src/runtime/vehicle-prediction.js';
import { captureGroundCorrection, restoreGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { captureFlightCorrection, restoreFlightCorrection } from '../src/runtime/network-entity-flight.js';
import { captureCompactFlight, restoreCompactFlight } from '../src/runtime/network-entity-flight-compact.js';
import { captureWeaponState, decodeWeaponState, validateWeaponState, applyValidatedWeaponState } from '../src/runtime/network-weapon-state.js';
import { reconRandom } from '../src/runtime/network-remote-world.js';
import { hullFromPack, hullCommand, hullObservation, assertPredictionClone } from '../tests/fixtures/prediction-hull-cases.js';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'source revision required: --revision or BFE_SOURCE_REVISION');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex'), loaded = new Map(), authenticated = [];
const packs = [];
for (const query of ['?map=nab2&side=rep', '?map=geo1&side=rep', '?map=hot1&side=all', '?map=bes1&side=all']) {
  const manifest = await contentManifest(query, async path => {
    const response = await fetch(path);
    if (!response.ok || !/\/vehicles\/([^/]+\/)?vehicles\.json$/.test(path)) return response;
    const bytes = await response.arrayBuffer(); loaded.set(path, bytes);
    return { ok: true, status: 200, arrayBuffer: async () => bytes };
  });
  const path = manifest.find(([name, hash]) => /\/vehicles\/([^/]+\/)?vehicles\.json$/.test(name) && hash !== 'missing')?.[0];
  assert.ok(path && loaded.has(path), 'authenticated vehicle pack');
  const bytes = loaded.get(path), data = JSON.parse(new TextDecoder().decode(bytes));
  const files = [[path, bytes.byteLength, sha256(new Uint8Array(bytes))]], buffers = {};
  for (const [key, file] of [['buffer', 'models.bin'], ['indexBuffer', 'models_index.bin'], ['animationBuffer', 'animations.bin']]) {
    const name = path.slice(0, path.lastIndexOf('/') + 1) + file, response = await fetch(name);
    if (response.status === 404 && key === 'animationBuffer' && !Object.keys(data.animations || {}).length) {
      buffers[key] = new ArrayBuffer(0); files.push([name, 0, 'missing']); continue;
    }
    assert.ok(response.ok, `missing vehicle buffer ${file}`);
    buffers[key] = await response.arrayBuffer(); files.push([name, buffers[key].byteLength, sha256(new Uint8Array(buffers[key]))]);
  }
  authenticated.push({ query, hash: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)), manifest, files });
  packs.push({ data, buffers, fingerprint: sha256(JSON.stringify(files.map(([, size, hash]) => [size, hash]))) });
}

const corpusPath = option('corpus') ? resolve(option('corpus')) : null;
if (corpusPath) {
  const parent = realpathSync(dirname(corpusPath)), physical = join(parent, basename(corpusPath));
  const within = relative(realpathSync(root), physical);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'content corpus must stay outside the checkout');
  for (let dir = parent; ; dir = dirname(dir)) {
    assert.equal(existsSync(join(dir, '.git')), false, 'content corpus must stay outside repositories');
    if (dir === dirname(dir)) break;
  }
  assert.equal(existsSync(corpusPath), false, 'content corpus already exists');
  assert.equal(existsSync(corpusPath + '.manifest.json'), false, 'content corpus manifest already exists');
}
const descriptor = corpusPath ? openSync(corpusPath, 'wx') : null;
const corpusHash = createHash('sha256'), stateHash = createHash('sha256'), kinds = {}, unique = new Set(), classHashes = [];
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
const ticksPerHull = 360, checkpointEvery = 6, operations = Array(8).fill(0);
let label = '', corpusBytes = 0, hulls = 0, checkpoints = 0, continuations = 0, observations = 0, weapons = 0, shots = 0, completed = false, report;
let native;

const entries = hull => hull.seats.flatMap((seat, seatIndex) => seat.weapons.map((entry, index) => ({ weapon: entry.state, key: `${seatIndex}:${index}` })));
function bindStreams(hull) {
  for (const { weapon, key } of entries(hull)) weapon.random = reconRandom(0x2486abcd, `correction:${key}`);
}
function advanceWeapons(hull, tick) {
  let count = 0;
  for (const { weapon } of entries(hull)) {
    const held = tick % 37 < 23, pressed = tick % 11 === 1;
    count += weapon.update(1 / 60, true, { held, pressed }, tick % 101 === 51,
      { posture: 'stand', stick: 0, zoomed: false }).length;
  }
  return count;
}
function advance(hull, controls, command, privateHull) {
  controls.accept({ ...command, epoch: 1, sequence: command.tick });
  const input = networkVehicleInput(hull, 0, controls, command), enabled = hull.body.enabled;
  try {
    hull.captureTick();
    if (hull.flying) hull.tick(1 / 60, input);
    else { hull.tick(1 / 120, input); hull.tick(1 / 120, input); }
    hull.walkFromPhase = hull.walkPhase; hull.walkFromBlend = hull.walkBlend; hull.walk(1 / 60);
    return advanceWeapons(hull, command.tick);
  } finally { if (privateHull) hull.body.enabled = enabled; }
}
function observation(hull) {
  return { hull: hullObservation(hull, false), weapons: entries(hull).map(({ weapon, key }) => ({ key,
    bytes: Array.from(captureWeaponState(weapon)), level: weapon.level, salvoSeed: weapon.salvoSeed,
    salvoCount: weapon.salvoCount, randomSeed: weapon.random.seed, seeded: Boolean(weapon.salvoRandom) })) };
}
function retained(hull) {
  const objects = Object.entries(hull).filter(([, value]) => value?.isVector3 || value?.isQuaternion || value?.isEuler || value?.isMatrix4);
  return { objects, seats: hull.seats, seat: hull.seats[0], wheels: hull.wheels, legSide: hull.legSide,
    walkPhase: hull.walkPhase, wheelPhases: hull.wheels.map(w => w.phase),
    weapons: entries(hull).map(({ weapon }) => ({ weapon, random: weapon.random, salvoRandom: weapon.salvoRandom, seed: weapon.salvoSeed, count: weapon.salvoCount })),
    collider: hull.body.matrix.toArray(), enabled: hull.body.enabled };
}
function checkRetained(hull, saved) {
  for (const [name, value] of saved.objects) assert.equal(hull[name], value, `${label} retained ${name}`);
  assert.equal(hull.seats, saved.seats); assert.equal(hull.seats[0], saved.seat); assert.equal(hull.wheels, saved.wheels); assert.equal(hull.legSide, saved.legSide);
  assert.equal(hull.walkPhase, saved.walkPhase); assert.deepEqual(hull.wheels.map(w => w.phase), saved.wheelPhases);
  for (const { weapon, random, salvoRandom, seed, count } of saved.weapons) {
    assert.equal(weapon.random, random); assert.equal(weapon.salvoRandom, salvoRandom); assert.equal(weapon.salvoSeed, seed); assert.equal(weapon.salvoCount, count);
  }
  assert.deepEqual(hull.body.matrix.toArray(), saved.collider); assert.equal(hull.body.enabled, saved.enabled);
  assertPredictionClone(hull, hull.predictionSource);
}
function framed(bytes, tick) {
  const offset = tick % 2 ? 1 : 7, storage = new Uint8Array(bytes.length + offset + 5).fill(0xa5);
  storage.set(bytes, offset); return storage.subarray(offset, offset + bytes.length);
}
function compare(pair, tick) {
  const expected = observation(pair.expected), actual = observation(pair.actual);
  assert.deepEqual(actual, expected, `${label} tick ${tick}`);
  stateHash.update(JSON.stringify(actual)); observations++;
  assert.deepEqual(pair.expected.body.matrix.toArray(), pair.expectedCollider);
  assert.deepEqual(pair.actual.body.matrix.toArray(), pair.actualCollider);
}
function restore(pair, authority, tick) {
  label = `${pair.label}:checkpoint-${tick}`;
  const payload = framed(pair.capture(authority), tick), sourceBefore = observation(authority), left = retained(pair.expected), right = retained(pair.actual);
  const before = native.calls;
  pair.restore(pair.expected, payload);
  pair.restore(pair.actual, payload, native);
  assert.equal(native.calls - before, 1, `${label} body reader delegates`);
  const sourceWeapons = entries(authority), expectedWeapons = entries(pair.expected), actualWeapons = entries(pair.actual);
  assert.equal(sourceWeapons.length, expectedWeapons.length); assert.equal(sourceWeapons.length, actualWeapons.length);
  for (let i = 0; i < sourceWeapons.length; i++) {
    const bytes = framed(captureWeaponState(sourceWeapons[i].weapon), tick + i);
    applyValidatedWeaponState(validateWeaponState(expectedWeapons[i].weapon, decodeWeaponState(bytes)));
    const calls = native.calls;
    applyValidatedWeaponState(validateWeaponState(actualWeapons[i].weapon, decodeWeaponState(bytes, native)));
    assert.equal(native.calls - calls, 1, `${label} weapon reader delegates`); weapons++;
  }
  checkRetained(pair.expected, left); checkRetained(pair.actual, right);
  assert.deepEqual(pair.capture(pair.actual), pair.capture(pair.expected), `${label} recapture`);
  assert.deepEqual(pair.capture(pair.actual), payload, `${label} exact transmitted state`);
  assert.deepEqual(observation(authority), sourceBefore, `${label} authority unchanged`);
  compare(pair, tick); checkpoints++;
}

try {
  native = createCorrectionSim(await loadSim(), { trace(event) {
    assert.equal(event.error, null, `${label} valid content correction`);
    const line = JSON.stringify({ label, op: event.op, input: words(event.input), output: words(event.output), error: event.error ?? null }) + '\n';
    corpusHash.update(line); corpusBytes += Buffer.byteLength(line); if (descriptor !== null) writeSync(descriptor, line); operations[event.op]++;
  } });
  for (const { data, buffers, fingerprint } of packs) for (const [name, def] of Object.entries(data.classes)) {
    if (!['hover', 'walker', 'commandwalker', 'flyer'].includes(def.base || 'hover')) continue;
    const digest = sha256(JSON.stringify([fingerprint, name, def, data.models[def.model], data.animations?.[def.model]]));
    if (unique.has(digest)) continue; unique.add(digest); classHashes.push(digest);
    const kind = def.base === 'flyer' ? 'flyer' : (def.settings?.HealthType || '').toLowerCase() === 'animal' ? 'creature'
      : ['walker', 'commandwalker'].includes(def.base) ? 'walker' : /hailfire/i.test(name) ? 'wheeled' : def.springs?.length ? 'springs' : 'hover';
    kinds[kind] = (kinds[kind] || 0) + 1;
    const hullLabel = `content-correction-${hulls++}`, pack = { ...data, classes: { [name]: def }, models: { [def.model]: data.models[def.model] },
      collision: { [def.model]: data.collision[def.model] }, animations: data.animations?.[def.model] ? { [def.model]: data.animations[def.model] } : {} };
    const fixtures = [], create = () => { const fixture = hullFromPack(pack, buffers, name); fixtures.push(fixture); bindStreams(fixture.v); return fixture; };
    try {
      const authority = create().v, authorityControls = new CommandControls();
      const modes = authority.flying ? [
        { name: 'flight-full', capture: captureFlightCorrection, restore: (v, b, backend) => restoreFlightCorrection(v, b, 2, backend) },
        { name: 'flight-compact', capture: captureCompactFlight, restore: restoreCompactFlight },
      ] : [{ name: 'ground', capture: captureGroundCorrection, restore: restoreGroundCorrection }];
      const pairs = modes.map(mode => {
        const expected = predictionVehicle(create().v), actual = predictionVehicle(create().v);
        assert.notEqual(expected.position, actual.position); assert.notEqual(expected.velocity, actual.velocity);
        assertPredictionClone(expected, expected.predictionSource); assertPredictionClone(actual, actual.predictionSource);
        return { ...mode, label: `${hullLabel}-${mode.name}`, expected, actual,
          expectedControls: new CommandControls(), actualControls: new CommandControls(), expectedCollider: expected.body.matrix.toArray(), actualCollider: actual.body.matrix.toArray() };
      });
      for (const pair of pairs) restore(pair, authority, 0);
      for (let tick = 1; tick <= ticksPerHull; tick++) {
        const command = hullCommand(tick, authority.flying);
        shots += advance(authority, authorityControls, command, false);
        for (const pair of pairs) {
          label = `${pair.label}:continue-${tick}`;
          const sourceBefore = observation(authority);
          const expectedShots = advance(pair.expected, pair.expectedControls, command, true), actualShots = advance(pair.actual, pair.actualControls, command, true);
          assert.equal(actualShots, expectedShots, `${label} continued weapon shots`);
          compare(pair, tick); continuations++;
          assert.deepEqual(observation(authority), sourceBefore, `${label} private continuation leaves authority untouched`);
          if (tick % checkpointEvery === 0) restore(pair, authority, tick);
        }
      }
    } finally { for (const fixture of fixtures) fixture.system.dispose(); }
  }
  assert.ok(hulls >= 16 && kinds.flyer > 0 && kinds.walker > 0 && (kinds.hover || kinds.springs || kinds.wheeled), 'authenticated hull variety');
  assert.ok(operations[4] >= 30 && operations[5] >= 30 && operations[6] >= 30 && native.calls >= 300, 'ground and both flight readers exercised');
  assert.ok(operations[0] > 0 && weapons > 0 && shots > 0, 'actual mounted weapon state and continuation exercised');
  report = { revision, content: authenticated, classHashes, hulls, kinds, ticksPerHull, checkpointEvery, checkpoints, continuations, observations,
    weaponRestores: weapons, authorityShots: shots, operations, totalOperations: native.calls,
    stateSha256: stateHash.digest('hex'), corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { native?.dispose(); if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
