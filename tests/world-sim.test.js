import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Vector3 } from 'three';
import { ClassicRandom } from '../src/ai/random.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadSim } from '../src/sim/module.js';

// Execute the source pass itself, so the native bridge is checked against
// the live reference rather than a second transcription of its arithmetic.
const source = readFileSync(new URL('../src/game.js', import.meta.url), 'utf8');
const begin = source.indexOf('  const MAIN_BODY = ');
const end = source.indexOf('  const localWorldHost = {', begin);
assert.ok(begin >= 0 && end > begin);
const reference = new Function('units', 'playerUnit', 'scope', `
  const game = { worldSim: null }, driver = { active: false }, gunner = { active: false }, combat = { dead: scope.playerOut };
  const simulationCamera = { position: scope.camera };
  ${source.slice(begin, end)}
  separationScope.global = scope.global;
  separateSoldiers();
`);
const unit = (id, x, y, z, other = {}) => ({ id, position: new Vector3(x, y, z), tickCount: 0, soldier: true,
  seat: null, held: null, removed: false, alive: true, dead: false, walker: false, hitPosture: 'stand', ...other });
const copy = members => members.map(value => ({ ...value, position: value.position.clone() }));
const state = members => members.map(value => [value.id, ...value.position.toArray(), value.tickCount]);

test('native crowd separation twins the live source over mixed bodies, scopes and reused slots', async () => {
  const instance = await loadSim(), sim = createWorldSim(instance);
  assert.equal(sim.bridge, bridgeFor(instance));
  let passes = 0;
  for (const seed of [1, 7, 79, 911, 4321]) {
    const random = new ClassicRandom(seed), pick = values => values[Math.floor(random.uniform() * values.length)];
    const members = Array.from({ length: 40 }, (_, i) => unit(i * 3 + 1, 0, 0, 0));
    for (let pass = 0; pass < 80; pass++) {
      for (const [i, value] of members.entries()) {
        value.position.set((i % 8) * 0.8 + random.range(-1, 1), pick([0, 0, 0.2, 0.5, 2]), Math.floor(i / 8) * 0.7 + random.range(-1, 1));
        Object.assign(value, { soldier: i % 13 !== 0, seat: pick([null, null, null, {}]), held: pick([null, null, null, {}]),
          removed: i % 17 === pass % 17, alive: i % 3 !== pass % 3, dead: i % 11 !== 0, walker: pick([false, false, true]),
          hitPosture: pick(['stand', 'crouch', 'prone', 'unknown']), tickCount: random.int(0, 90) });
      }
      const js = copy(members), native = copy(members);
      const options = { playerOut: pass % 4 === 0, global: pass % 3 === 0,
        camera: new Vector3(pick([0, 60, 66, -60]), 0, 0) };
      reference({ all: () => js }, js[1], options);
      sim.separate(native, { ...options, player: native[1] });
      assert.deepEqual(state(native), state(js), `seed ${seed}, pass ${pass}`);
      passes++;
    }
  }
  assert.equal(passes, 400);
});

test('native crowd contacts preserve coincident-body direction, height boundaries and public positions', async () => {
  const instance = await loadSim(), sim = createWorldSim(instance);
  for (const dx of [0, -0, 0.00001, 0.0001, 1.14, 1.139999999999, 2]) {
    for (const height of [0, 0.45, 1.65, 2, -0]) {
      const js = [unit(17, 0, 0, 0), unit(19, dx, height, 0), unit(23, 0, 0, 0, { walker: true })];
      const native = copy(js), positions = native.map(u => u.position), options = { global: true, camera: new Vector3() };
      reference({ all: () => js }, null, options);
      instance.exports.memory.grow(1);
      sim.separate(native, options);
      assert.deepEqual(state(native), state(js));
      for (let i = 0; i < native.length; i++) assert.equal(native[i].position, positions[i]);
      // The shared native pool may service another caller between ticks;
      // every eligibility and body field is refreshed for the next pass.
      createWorldSim(instance).separate([unit(17, 50, 0, 0, { walker: true, held: {} })], options);
      reference({ all: () => js }, null, options); sim.separate(native, options);
      assert.deepEqual(state(native), state(js));
    }
  }
});
