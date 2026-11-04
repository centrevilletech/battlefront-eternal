// Validate actual inventory and mounted corrections on the live bridge. This
// observer changes no actor state; optional native transcripts stay external.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { root } from './headless-world.mjs';
import { capturePlayerCorrection } from '../src/runtime/network-entity-capture.js';
import { decodeOwnPlayer } from '../src/runtime/network-entity-player.js';
import { validateArsenalWeapon } from '../src/runtime/network-arsenal-state.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
import { validateMountedControl } from '../src/runtime/network-mounted-control.js';
import { createCorrectionBridge } from '../src/runtime/correction-sim.js';
import { contentManifest } from '../src/runtime/content-hash.js';

const sha256 = value => createHash('sha256').update(value).digest('hex');
const revision = process.env.BFE_SOURCE_REVISION || process.env.BFE_COMMIT;
assert.match(revision || '', /^[a-f0-9]{40}$/, 'live reader proof requires an explicit source revision');
const corpusDirectory = process.env.BFE_STATE_READERS_DIR ? realpathSync(resolve(process.env.BFE_STATE_READERS_DIR)) : null;
if (corpusDirectory) {
  const within = relative(realpathSync(root), corpusDirectory);
  assert.ok(isAbsolute(within) || within === '..' || within.startsWith('../'), 'reader corpus must stay outside the checkout');
  for (let directory = corpusDirectory; ; directory = dirname(directory)) {
    assert.equal(existsSync(join(directory, '.git')), false, 'reader corpus must stay outside repositories');
    if (directory === dirname(directory)) break;
  }
}
const corpusHash = createHash('sha256'), payloadHash = createHash('sha256');
const modes = {}, classes = {}, opcodes = {}, names = new Set(), queries = new Set();
let current = null, descriptor = null, corpusPath = null, label = '', samples = 0, calls = 0, bytes = 0, battles = 0;
let inventory = 0, mounted = 0, mountedWeapons = 0, melee = 0;
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
function trace(event) {
  assert.equal(event.error, null, 'published weapon correction is valid');
  const line = JSON.stringify({ label, op: event.op, input: words(event.input), output: words(event.output), error: null }) + '\n';
  corpusHash.update(line); bytes += Buffer.byteLength(line); calls++;
  opcodes[event.op] = (opcodes[event.op] || 0) + 1;
  if (descriptor !== null) writeSync(descriptor, line);
}
function compareEntry(actual, expected, weapon) {
  assert.equal(actual.weapon, weapon, `${label} native weapon identity`);
  assert.equal(expected.weapon, weapon, `${label} JavaScript weapon identity`);
  assert.deepEqual(actual.state, expected.state, `${label} weapon state`);
  assert.equal(JSON.stringify(actual.state), JSON.stringify(expected.state), `${label} weapon property order`);
  assert.deepEqual(actual.melee, expected.melee, `${label} melee state`);
  names.add(weapon.name);
  if (actual.melee) melee++;
}

export function battle({ game, server, world, index, options }) {
  assert.equal(current, null);
  assert.ok(game.simBridge, 'live reader proof requires the shared simulation bridge');
  if (!battles && corpusDirectory) {
    const key = sha256(JSON.stringify(options)).slice(0, 16);
    corpusPath = join(corpusDirectory, `readers-${options.map}-${options.side}-${key}.jsonl`);
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
  const before = Uint8Array.from(record.payload), data = decodeOwnPlayer(record);
  const unit = world.players.get(server.id).unit, weapons = unit.combat.arsenal.weapons;
  label = `battle-${index + 1}-tick-${tick}`;
  assert.equal(data.weapons.length, weapons.length);
  for (let i = 0; i < weapons.length; i++) {
    const weapon = weapons[i], payload = data.weapons[i].payload, stateBefore = captureWeaponState(weapon);
    const definition = weapon.def, levels = weapon.levels, first = native.calls;
    let expected, actual;
    if (samples % 2) { actual = validateArsenalWeapon(weapon, payload, native); expected = validateArsenalWeapon(weapon, payload); }
    else { expected = validateArsenalWeapon(weapon, payload); actual = validateArsenalWeapon(weapon, payload, native); }
    assert.ok(native.calls - first > 1, `${label} native delta parsing`);
    compareEntry(actual, expected, weapon);
    assert.equal(weapon.def, definition); assert.equal(weapon.levels, levels);
    assert.deepEqual(captureWeaponState(weapon), stateBefore, `${label} authority unchanged`);
    inventory++;
  }
  if (data.mounted) {
    const first = native.calls, expected = validateMountedControl(unit, data.mounted), actual = validateMountedControl(unit, data.mounted, native);
    assert.ok(native.calls > first, `${label} native mounted parsing`);
    assert.equal(actual.target, expected.target); assert.deepEqual(actual.state, expected.state);
    assert.equal(actual.weapons.length, expected.weapons.length);
    for (let i = 0; i < actual.weapons.length; i++) {
      compareEntry(actual.weapons[i], expected.weapons[i], expected.weapons[i].weapon);
      assert.equal(actual.weapons[i].owner, expected.weapons[i].owner);
      assert.equal(actual.weapons[i].barrel, expected.weapons[i].barrel);
      mountedWeapons++;
    }
    mounted++;
  }
  assert.deepEqual(new Uint8Array(record.payload.buffer, record.payload.byteOffset, record.payload.byteLength), before, `${label} source bytes unchanged`);
  modes[data.controlMode] = (modes[data.controlMode] || 0) + 1;
  classes[unit.className] = (classes[unit.className] || 0) + 1;
  payloadHash.update(record.payload); samples++;
}

export function ended() {
  current.native.dispose(); current = null;
}

export async function finish() {
  assert.equal(current, null);
  assert.ok(samples > 0 && inventory > 0 && Object.keys(opcodes).some(op => Number(op) > 8), 'live corrections reached staged native parsing');
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
  const report = { closed: true, revision, battles, samples, inventory, mounted, mountedWeapons, melee, calls, modes, classes,
    weapons: [...names].sort(), opcodes, authentication, payloadSha256: payloadHash.digest('hex'),
    corpusBytes: bytes, corpusSha256: corpusHash.digest('hex') };
  if (corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  console.error(JSON.stringify({ stateReaderProof: { revision, battles, samples, inventory, mounted, mountedWeapons, melee, calls, opcodes } }));
}
