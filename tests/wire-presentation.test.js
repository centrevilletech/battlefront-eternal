import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createWirePresentation } from '../src/runtime/wire-presentation.js';
import { captureUnitState } from '../src/runtime/entity-unit-state.js';
import { captureWorldPresentation } from '../src/runtime/entity-world-state.js';
import { presentationRecord, attachPresentationCapture } from '../src/runtime/entity-presentation-events.js';
import { emitSound } from '../src/audio/events.js';

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

test('presentation event JSON preserves entry order, vector holes, UTF-16 and visual damage clamping', async () => {
  const instance = await loadSim(), native = createWirePresentation(instance), u = random(2281);
  const kinds = ['weaponAction', 'endWeaponAction', 'shot', 'remoteShot', 'shieldHit', 'deathBlast', 'blowApart', 'remoteBlast', 'effect', 'lightning', 'vo', 'bounce', 'pickup', 'serve', 'towAttach', 'towRelease', 'squad'];
  const texts = ['雪😀\ud800\udfff', '"\\\n\u0000\u2028\u2029', null, '', 'reload'];
  try {
    for (let i = 0; i < 1800; i++) {
      const pairs = [['kind', kinds[i % kinds.length]], ['amount', i % 7 === 0 ? -Infinity : (u() - 0.5) * 1000],
        ['position', i % 3 ? [u() * 1e-7, -0, 1000000000000000100] : null], ['name', texts[i % texts.length]],
        ['ownerId', i % 3 ? 0xffffffff : -0], ['ownerGeneration', i], ['channel', i % 2], ['human', i % 2 === 0], ['prepare', false]];
      if (pairs[0][1] !== 'blowApart' && !Number.isFinite(pairs[1][1])) pairs[1][1] = -Number.MAX_VALUE;
      for (let j = pairs.length - 1; j > 0; j--) { const k = Math.floor(u() * (j + 1)); [pairs[j], pairs[k]] = [pairs[k], pairs[j]]; }
      const event = Object.fromEntries(pairs), options = { id: i, generation: 4, tick: 99, event };
      if (i % 197 === 0) instance.exports.memory.grow(1);
      assert.deepEqual(presentationRecord({ ...options, native }), presentationRecord(options), `event ${i}`);
    }
    for (const event of [
      { kind: 'blowApart', amount: -Infinity }, { kind: 'blowApart', amount: null }, { kind: 'blowApart', amount: '-100' },
      { kind: 'blowApart', amount: false }, { kind: 'blowApart', amount: [] }, { kind: 'blowApart', amount: { valueOf: () => -90 } },
      { kind: 'shot', position: Array(3) }, { kind: 'shot', position: [, -0, ] },
      Object.create({ kind: 'shot' }, { amount: { value: 1, enumerable: true } }),
    ]) {
      // A sparse vector must still have the three entries the validator expects.
      if (event.position) event.position.length = 3;
      assert.deepEqual(presentationRecord({ event, native }), presentationRecord({ event }));
    }
    const event = { kind: 'blowApart', amount: { valueOf() { instance.exports.memory.grow(1); native.world([1e-7, '雪'], 17); return -Infinity; } } };
    assert.deepEqual(presentationRecord({ event, native }), presentationRecord({ event }), 'foreign damage coercion completes before native state is borrowed');
  } finally { native.dispose(); }
});

test('presentation event failures retain field validation order and exact error text', async () => {
  const native = createWirePresentation(await loadSim());
  const invalid = [undefined, null, {}, { kind: 'unknown' }, { kind: 'shot', name: 1 }, { kind: 'shot', name: '😀'.repeat(65) },
    { kind: 'shot', position: [0, 0, undefined] }, { kind: 'shot', position: [0, 0, NaN] }, { kind: 'shot', position: [0, 0] },
    { kind: 'shot', position: { x: 0, y: 0, z: 0 } }, { kind: 'shot', channel: '0' }, { kind: 'shot', channel: 2 },
    { kind: 'shot', ownerId: -1 }, { kind: 'shot', ownerId: 4294967296 }, { kind: 'shot', ownerId: '1' },
    { kind: 'shot', amount: Infinity }, { kind: 'shot', amount: null }, { kind: 'shot', human: 1 }, { kind: 'shot', end: null },
    { kind: 'shot', 'unknown\ud800': 1 }, { amount: NaN, kind: 'unknown' }, { amount: NaN, kind: 'shot', ownerId: -1 },
    { ownerId: -1, kind: 'shot', amount: NaN }, { kind: 'blowApart' }, { kind: 'blowApart', amount: Infinity },
    Object.create({ kind: 'blowApart', amount: -Infinity }),
    Object.fromEntries(['kind', 'name', 'action', 'bank', 'posture', 'hitKind', 'line', 'reason', 'label', 'property'].map(key => [key, key === 'kind' ? 'shot' : '雪'.repeat(128)]))];
  try {
    for (const event of invalid) {
      let expected; try { presentationRecord({ event }); } catch (error) { expected = error; }
      assert.ok(expected, 'reference rejects event');
      assert.throws(() => presentationRecord({ event, native }), error => error.constructor === expected.constructor && error.message === expected.message);
    }
  } finally { native.dispose(); }
});

test('live presentation listener uses native bytes for simulation and observed sound events', async () => {
  const native = createWirePresentation(await loadSim()), registry = { weaponIds: new Map([['rifle', 1]]), combat: { definitionId: () => 8 } };
  const fixture = encoder => {
    let listener; const records = [], faults = [], building = {}, supply = {}, vehicle = { aiTarget: { id: 29, generation: 3 } };
    const game = { simEvents: { listen(fn) { listener = fn; return () => { listener = null; }; } }, live: { buildings: [building] }, units: { all: () => [{ id: 7, generation: 2, supply }] }, vehicles: { vehicles: [vehicle] } };
    const capture = { nextId: 1000, generation: 2, tick: () => 19, emit: record => records.push(record), fault: error => faults.push(error.message) };
    const dispose = attachPresentationCapture(game, capture, registry, encoder);
    return { records, faults, dispose, feed: event => listener(event), building, supply, vehicle };
  };
  const a = fixture(null), b = fixture(native);
  try {
    const source = { generation: 3, unit: { id: 7, combat: { arsenal: { weapons: [{ name: 'rifle', channel: 0 }] } } } };
    for (const event of [
      { kind: 'presentation', event: { kind: 'blowApart', amount: -Infinity, ownerId: 7, ownerGeneration: 3 } },
      { kind: 'vo', source, team: 2, position: [1, 2, 3], human: true, property: 'radio', positional: false },
      { kind: 'shot', source, weapon: 'rifle', position: [3, 2, 1], repair: false, melee: false, swing: 0 },
      { kind: 'remoteShot', droid: {}, channel: 1, position: [3, 2, 1] },
      { kind: 'presentation', event: { kind: 'effect', position: [0, 0, NaN] } },
    ]) { a.feed(event); b.feed(event); }
    emitSound('bounce', {}, [4, 5, 6], 0.3); emitSound('pickup', {}, [7, 8, 9], 2);
    assert.deepEqual(b.records, a.records); assert.deepEqual(b.faults, a.faults); assert.equal(b.records.length, 6); assert.equal(b.faults.length, 1);
  } finally { a.dispose(); b.dispose(); native.dispose(); }
});
