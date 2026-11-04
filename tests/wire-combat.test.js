import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3, Quaternion } from 'three';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { WireCombat } from '../src/runtime/wire-combat.js';
import { CombatCapture, attachCombatCapture, combatRecord } from '../src/runtime/network-entity-combat.js';
import { presentationRecord } from '../src/runtime/entity-presentation-events.js';
import { SimEvents } from '../src/sim-events.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { combatCases, combatEncoders } from '../tools/wire-combat-fixtures.mjs';

test('all combat producer payloads match JavaScript bytes and validation order', async () => {
  const native = new WireCombat({ instance: await loadSim() });
  try {
    for (const { encoder, value, label } of combatCases()) {
      const encode = combatEncoders[encoder]; let expected, error;
      try { expected = encode(value); } catch (e) { error = e; }
      if (error) assert.throws(() => encode({ ...value, native }), e => e.message === error.message, `${label}: ${error.message}`);
      else assert.deepEqual(encode({ ...value, native }), expected, label);
    }
  } finally { native.dispose(); }
});

function fixture(bridge = null, nested = null) {
  let tick = 10;
  const calls = [], ordnance = { grenades: [], rounds: [], beams: [],
    onLaunch: () => calls.push('launch'), onImpact: () => calls.push('impact'), onExplode: () => calls.push('explode'), onProjectile: () => calls.push('projectile') };
  const simEvents = new SimEvents();
  const capture = new CombatCapture({ bridge, tick: () => { calls.push(`tick:${tick}`); return tick; }, definitionId(def) {
    calls.push(`definition:${def.id}`); nested?.(); if (def.id === 13) throw Error('unregistered definition'); return def.id;
  } });
  const detach = attachCombatCapture({ ordnance, simEvents }, capture);
  return { capture, calls, ordnance, simEvents, tick(value) { tick = value; }, dispose() { detach(); capture.dispose(); } };
}
function step(f, i) {
  f.tick(i + 10);
  const bolt = { def: { id: i % 19 === 0 ? 13 : 1 }, life: 0.7, speed: 61.3, merge: 8,
    position: new Vector3(i, 2, -0), direction: new Vector3(0, 0, 1), offset: new Vector3(-0.1, 0.2, 0.3),
    networkFireUnit: 7, networkFireGeneration: 2, networkFireId: i, networkFireChannel: i & 1 };
  f.ordnance.onLaunch(bolt, { tick: i + 9.25 });
  if (i % 3) f.ordnance.onImpact(bolt, { point: new Vector3(i, 2, 0.7), normal: new Vector3(0, 0, -1), surface: i % 6 });
  if (i % 2) f.ordnance.onExplode({ id: 2 }, new Vector3(-0, i, -i), null, { wet: i % 3 === 0 });
  f.simEvents.emit(i % 5 ? 'damage' : 'death', { target: { unit: { id: 31 }, generation: 3 }, attacker: { unit: { id: 7 }, generation: 2 },
    position: new Vector3(i, 0, 2), source: new Vector3(-i, 0, -2), team: 2, attackerTeam: 1 });
  f.capture.emit(presentationRecord({ id: f.capture.nextId++, generation: 1, tick: i + 10, event: { kind: 'pickup', name: 'item', amount: i } }));
  const type = ['grenade', 'round', 'beam'][i % 3], list = f.ordnance[`${type}s`];
  const p = { born: i, def: { id: 3 }, position: new Vector3(i, -i, 0), velocity: new Vector3(1, 2, 3), quaternion: new Quaternion(),
    end: new Vector3(i, -i, 5), life: 0.5, owner: { id: 7, generation: 2 }, networkFireChannel: i & 1 };
  list.push(p); f.ordnance.onProjectile(p, type);
  for (const key of ['grenades', 'rounds', 'beams']) {
    for (const p of f.ordnance[key]) { p.position.x += 0.01; p.life -= 0.01; p.gone = i - p.born > 4; }
    f.ordnance[key] = f.ordnance[key].filter(p => i - p.born <= 5);
  }
  if (i % 17 === 0) f.capture.fault({ message: `${i}:` + '😀'.repeat(100) });
}

test('native combat publication matches live producers, dynamic removals and nested bridge calls', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  let entered = false;
  const native = fixture(bridge, () => { wire.encodeWordDelta(new Uint8Array([9, 3, 2])); if (!entered) { entered = true; instance.exports.memory.grow(1); } });
  const js = fixture();
  try {
    for (let i = 1; i <= 180; i++) {
      step(native, i); step(js, i);
      if (i % 3 === 0) assert.deepEqual(native.capture.drain(), js.capture.drain(), `drain ${i}`);
      assert.deepEqual([...native.capture.pending], [...js.capture.pending], `pending ${i}`);
      assert.equal(native.capture.nextId, js.capture.nextId, `identity ${i}`);
      assert.deepEqual(native.capture.faults, js.capture.faults, `faults ${i}`);
      assert.deepEqual(native.calls, js.calls, `callback order ${i}`);
      if (i % 29 === 0) assert.deepEqual(native.capture.drainFaults(), js.capture.drainFaults());
    }
  } finally { native.dispose(); js.dispose(); }
  assert.throws(() => native.capture.drain(), /disposed/);
});

test('native queue prunes before decoding, retains terminal pairs and bounds diagnostics', async () => {
  const native = fixture(bridgeFor(await loadSim())), js = fixture();
  const launch = id => combatRecord({ id, generation: 1, tick: 10, schema: 1, expiryTick: 30, definitionId: 1,
    position: [0, 0, 0], direction: [0, 0, 1], life: 0.1, speed: 60, merge: 8 });
  try {
    for (const f of [native, js]) {
      f.capture.maxRecords = 1; f.capture.emit(launch(1));
      assert.throws(() => f.capture.emit({ id: 2, schema: 2 }), /capacity exceeded/);
      assert.throws(() => f.capture.emit({ id: 1, schema: 2 }), /capacity exceeded/);
      f.capture.maxRecords = 2;
      const terminal = combatRecord({ id: 1, generation: 1, tick: 10, schema: 2, expiryTick: 30, definitionId: 1, position: [0, 0, 0], direction: [0, 0, 1] });
      f.capture.emit(terminal); assert.deepEqual([...f.capture.pending.keys()], [1, -1]);
      f.tick(31); assert.throws(() => f.capture.emit({ id: 2, schema: 2 }), /invalid combat payload/);
      assert.equal(f.capture.pending.size, 0); assert.equal(f.capture.prunedTick, 31);
      for (let i = 0; i < 12; i++) f.capture.fault(`${i}:${'a'.repeat(157)}😀`);
      f.capture.nextId = 0x100000000;
      assert.throws(() => f.capture.event({ kind: 'death' }), /identity exhausted/);
    }
    assert.deepEqual(native.capture.drainFaults(), js.capture.drainFaults());
    assert.deepEqual(native.capture.drain(), js.capture.drain());
  } finally { native.dispose(); js.dispose(); }
});
