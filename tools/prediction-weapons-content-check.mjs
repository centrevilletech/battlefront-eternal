// Content-backed private weapon replay. Definitions are loaded through the
// headless fetch helper and authenticated by the runtime gameplay manifest.
// Only hashes and counts are printed. --corpus writes an owned native trace
// outside the checkout, with a closed checksum manifest beside it.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { openSync, writeSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, relative, isAbsolute, dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './headless-world.mjs';
import { contentManifest } from '../src/runtime/content-hash.js';
import { loadSim } from '../src/sim/module.js';
import { createWeaponPredictionSim } from '../src/runtime/weapon-prediction-sim.js';
import { runWeaponPredictionCase } from '../tests/fixtures/prediction-weapon-cases.js';

const args = process.argv.slice(2), option = name => { const i = args.indexOf(`--${name}`); return i < 0 ? null : args[i + 1]; };
let revision = option('revision') || process.env.BFE_SOURCE_REVISION;
if (!revision) try { revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch {}
assert.match(revision || '', /^[a-f0-9]{40}$/, 'source revision required: --revision or BFE_SOURCE_REVISION');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const authenticated = [], unitBytes = new Map();
const fingerprintFetch = async path => {
  const response = await fetch(path);
  if (!response.ok || !/^\/sides\/[^/]+\/units\.json$/.test(path)) return response;
  const bytes = await response.arrayBuffer(); unitBytes.set(path, bytes);
  return { ok: true, status: 200, arrayBuffer: async () => bytes };
};
for (const query of ['?map=nab2&side=rep', '?map=nab2&side=all']) {
  const manifest = await contentManifest(query, fingerprintFetch);
  authenticated.push({ query, hash: 'bfe-content-sha256-' + sha256(JSON.stringify(manifest)), manifest });
}
const sources = ['rep', 'cis', 'all', 'imp'].map(side => {
  const path = `/sides/${side}/units.json`;
  assert.ok(authenticated.some(m => m.manifest.some(([name, hash]) => name === path && hash !== 'missing')), `authenticated ${side} loadouts`);
  assert.ok(unitBytes.has(path)); return { side, data: JSON.parse(new TextDecoder().decode(unitBytes.get(path))) };
});

const tick = (n, actions = [], down = [], extra = {}) => ({ op: 'step', tick: n, command: { sequence: n, tick: n, generation: 3, actions, down, look: [] }, ...extra });
function scenario(definitions, index) {
  let at = 0;
  const operations = [{ op: 'unit', id: index + 101, generation: 1 + index % 5 }, { op: 'begin', tick: 0, epoch: 2 }];
  for (const channel of [0, 1]) {
    const weapons = definitions.filter(w => w.channel === channel);
    for (let i = 0; i < weapons.length; i++) {
      const def = weapons[i], action = channel ? 'fireSecondary' : 'firePrimary', key = channel ? 'Mouse2' : 'Mouse0';
      if (i) operations.push(tick(++at, [channel ? 'nextSecondary' : 'nextPrimary']));
      operations.push(tick(++at, ['zoom']));
      const held = Math.min(360, Math.max(24, Math.ceil((def.MaxPressedTime || 0) * 60) + 2));
      const stage = Array.from({ length: held }, (_, j) => tick(++at, j === 0 ? [action] : [], [key], {
        soldier: { posture: ['stand', 'crouch', 'prone'][(index + i) % 3], air: null, dive: 0, lock: 0, rolled: false },
        combat: { gunUnderwater: false }, presses: channel ? [null, 0x80000000 + at] : [at * 97, null],
      }));
      const cut = 4, checkpoint = `${channel}:${i}`;
      operations.push(...stage.slice(0, cut), { op: 'checkpoint', key: checkpoint }, ...stage.slice(cut, cut + 3),
        { op: 'restore' }, { op: 'begin', tick: stage[cut - 1].tick, epoch: 2, checkpoint }, ...stage.slice(cut));
      const duration = Math.max((def.InitialSalvoDelay || 0) + (def.ShotDelay || 0) + (def.SalvoCount || 1) * (def.SalvoDelay || 0),
        ...(def.swings || []).map(s => s.duration), 0.1);
      for (let j = 0; j < Math.min(360, Math.ceil(duration * 60) + 3); j++) operations.push(tick(++at));
      operations.push(tick(++at, ['reload']));
      for (let j = 0; j < Math.min(360, Math.ceil((def.ReloadTime || 0) * 60) + 3); j++) operations.push(tick(++at));
    }
  }
  operations.push({ op: 'restore' });
  return { name: `content-loadout-${index}`, definitions, operations };
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
const descriptor = corpusPath ? openSync(corpusPath, 'wx') : null;
const corpusHash = createHash('sha256'), stateHash = createHash('sha256');
let operations = 0, corpusBytes = 0, label = '', loadouts = 0, observations = 0, shots = 0, presented = 0;
const bases = new Set();
const words = values => { const bytes = Buffer.alloc(values.length * 8); for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8); return bytes.toString('hex'); };
let completed = false, report;
try {
  const native = createWeaponPredictionSim(await loadSim(), { trace(event) {
    const line = JSON.stringify({ label, session: String(event.session), op: event.op, input: words(event.input), output: words(event.output), error: event.error ?? null }) + '\n';
    corpusHash.update(line); corpusBytes += Buffer.byteLength(line); if (descriptor !== null) writeSync(descriptor, line); operations++;
  } });
  for (const { data } of sources) for (const def of Object.values(data.units)) {
    if (!def.weapons?.length) continue;
    for (const weapon of def.weapons) bases.add(weapon.base);
    const c = scenario(def.weapons, loadouts++); label = c.name;
    let actual, expected;
    try {
      actual = runWeaponPredictionCase(c, { native, after({ prediction, operation }) { if (operation.op === 'begin') assert.ok(prediction.nativeSession?.handle); } });
      expected = runWeaponPredictionCase(c);
      assert.deepEqual(actual.observations, expected.observations, label);
      assert.equal(actual.unit.combat.arsenal, actual.authority, `${label} authority restored`);
      for (const o of actual.observations) {
        stateHash.update(JSON.stringify(o)); observations++; shots += o.shots.length; presented += o.emitted.length;
        assert.deepEqual(o.authority, actual.observations[0].authority, `${label} authority state unchanged`);
      }
    } finally { actual?.prediction.dispose(); expected?.prediction.restore(); expected?.prediction.dispose?.(); }
  }
  assert.ok(loadouts > 0); assert.ok(shots > 0); assert.ok(operations > observations / 2);
  report = { revision, content: authenticated, loadouts, observations, returnedShots: shots, presentedShots: presented,
    bases: [...bases].sort(), stateSha256: stateHash.digest('hex'), operations, corpusBytes, corpusSha256: corpusHash.digest('hex') };
  completed = true;
} finally { if (descriptor !== null) closeSync(descriptor); }
if (completed && corpusPath) writeFileSync(corpusPath + '.manifest.json', JSON.stringify({ ...report, closed: true }, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(report, null, 2));
