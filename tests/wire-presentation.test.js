import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createWirePresentation } from '../src/runtime/wire-presentation.js';
import { captureUnitState } from '../src/runtime/entity-unit-state.js';
import { captureWorldPresentation } from '../src/runtime/entity-world-state.js';

const NUMBERS = ['heat', 'wait', 'reloadLeft', 'throwTime', 'charge', 'held', 'lockElapsed'];
const FLAGS = ['overheated', 'released', 'hidden', 'charging', 'lockComplete'];
function random(seed) { return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; }; }
function fixture(count = 12) {
  const weapons = Array.from({ length: count }, (_, i) => {
    const levels = [{}, {}, {}]; return { name: `weapon${i}`, levels, def: levels[i % 3] };
  });
  const arsenal = { weapons, current: channel => weapons[channel] };
  return {
    unit: { id: 19, generation: 3, hero: false, targetable: true, actor: { root: { visible: true } }, deadTime: 0,
      soldier: { stateTime: 0 }, character: { rosterSlot: 37 }, combat: { arsenal, repairLock: '', blowApart: false } },
    registry: { weaponIds: new Map(weapons.map((w, i) => [w.name, i + 1])) },
  };
}

test('unit presentation matches byte for byte across timers, flags and wrapping integers', async () => {
  const instance = await loadSim(), native = createWirePresentation(instance), { unit, registry } = fixture();
  const u = random(3928), choose = values => values[Math.floor(u() * values.length)];
  try {
    for (let i = 0; i < 1200; i++) {
      unit.hero = u() < 0.5; unit.targetable = u() < 0.5; unit.actor.root.visible = u() < 0.5; unit.combat.blowApart = u() < 0.5;
      unit.deadTime = choose([0, -0, undefined, NaN, u() * 30]); unit.soldier.stateTime = u() * 3;
      unit.character.rosterSlot = choose([undefined, null, -1, 65537, 0, 918]);
      unit.combat.repairLock = choose(['', 'repair:vehicle', 'a\ud800b\udfff', '雪😀\u0000\n']);
      for (const w of unit.combat.arsenal.weapons) {
        for (const field of FLAGS) w[field] = u() < 0.5;
        for (const field of NUMBERS) w[field] = choose([undefined, null, -0, 0, 1, -1, u() * 50 - 20]);
        w.lockTargetId = choose([null, undefined, -1, 0, 4294967297, 300]);
        w.lockGeneration = choose([null, undefined, -1, 0, 65537, 47]);
        w.def = choose(w.levels);
      }
      if (i % 97 === 0) instance.exports.memory.grow(1);
      const expected = captureUnitState(unit, registry, i), actual = captureUnitState(unit, registry, i, native);
      assert.deepEqual(actual, expected, `unit ${i}`);
      assert.equal(captureUnitState(unit, registry, i + 1, native).payload, actual.payload);
    }
  } finally { native.dispose(); }
});

test('world JSON preserves the server number formatting, Unicode and array nulls', async () => {
  const instance = await loadSim(), native = createWirePresentation(instance), u = random(9701), encode = new TextEncoder();
  const edge = [-0, NaN, Infinity, -Infinity, 0, Number.MIN_VALUE, Number.MAX_VALUE, 1e-7, 1e-6, 1e20, 1e21,
    1000000000000000100, '\ud800', '\udc00', '\ud800\udc00', '"\\\b\f\n\r\t\u0000\u001f\u2028\u2029😀雪', null, undefined, true, false];
  try {
    for (let i = 0; i < 1600; i++) {
      const value = [edge[i % edge.length], edge[Math.floor(u() * edge.length)], [u() * 1e21, u() * 1e-8, (u() - 0.5) * 300]];
      if (i % 211 === 0) instance.exports.memory.grow(1);
      const actual = native.world(value, i % 37);
      assert.deepEqual(actual, encode.encode(JSON.stringify(value)), `world ${i}`);
      assert.equal(native.world(value, i % 37), actual);
    }
    assert.throws(() => native.world(['x'.repeat(901)], 1), /World presentation record too large/);
  } finally { native.dispose(); }
  assert.throws(() => native.world([], 1), /disposed/);
});

test('live world capture routes stations, creatures, traps and tows through native bytes', async () => {
  const instance = await loadSim(), native = createWirePresentation(instance);
  const a = { alive: true, aiTarget: { id: 31, generation: 4 }, towTarget: 1, towLocal: { toArray: () => [-0, 1e-7, 1000000000000000100] }, towLeg: 'rider\ud800雪', towAngle: 1.234, towWinding: -5 };
  const b = { alive: true, aiTarget: { id: 63, generation: 8 }, towTarget: -1 };
  const building = { state: 'built', health: Infinity, team: 2, mode: 'waiting', active: 1, pulse: 2.3, facing: -1, yaw: 1e-7, servedHealth: true };
  const game = { live: { buildings: [building], animated: [{ grabs: [{}], slot: 0, state: () => [1, 0.3, 0, 91, 3, -1, 0] }], traps: [{ stateRecord: () => [2, 17, 1] }] }, vehicles: { vehicles: [a, b] } };
  try {
    const expected = captureWorldPresentation(game, 19, 2), actual = captureWorldPresentation(game, 19, 2, null, native);
    assert.deepEqual(actual, expected); assert.equal(actual.length, 5);
    const again = captureWorldPresentation(game, 20, 2, null, native);
    for (let i = 0; i < actual.length; i++) assert.equal(again[i].payload, actual[i].payload);
    const calls = [], held = (schema, id, team) => { calls.push([schema, id, team]); return schema === 7 ? actual[0] : undefined; };
    assert.deepEqual(captureWorldPresentation(game, 20, 2, held, native).map(r => r.payload), actual.map(r => r.payload));
    assert.deepEqual(calls, [[7, 200000, 'built:2'], [8, 300031, undefined], [8, 300063, undefined]]);
  } finally { native.dispose(); }
});

test('unit presentation failures preserve the byte writer validation', async () => {
  const native = createWirePresentation(await loadSim()), { unit, registry } = fixture(1), w = unit.combat.arsenal.weapons[0];
  const compareError = () => {
    let expected;
    try { captureUnitState(unit, registry, 1); } catch (error) { expected = error.message; }
    assert.ok(expected);
    assert.throws(() => captureUnitState(unit, registry, 1, native), error => error.message === expected);
  };
  try {
    for (const value of [NaN, Infinity, -Infinity, '1', true]) { w.heat = value; compareError(); }
    delete w.heat;
    unit.combat.repairLock = '雪'.repeat(43); compareError(); unit.combat.repairLock = '';
    registry.weaponIds.delete(w.name); compareError(); registry.weaponIds.set(w.name, 1);
    unit.deadTime = Infinity; compareError(); unit.deadTime = 0;
    unit.combat.arsenal.weapons = Array(13).fill(w); compareError();
  } finally { native.dispose(); }
});
