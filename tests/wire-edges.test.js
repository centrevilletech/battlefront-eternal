import test from 'node:test';
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { EntityEdges } from '../src/runtime/network-entity-edge-host.js';
import { INPUT_CODES, INPUT_ACTIONS, encodeEntityInput, encodeSnapshotAckKeepalive } from '../src/runtime/network-entity-input.js';

function fixture(bridge = null, callback = null) {
  const mailbox = () => ({ received: 0, outcomes: [], enableOutcomes() {}, drainOutcomes() { return this.outcomes.splice(0); } });
  const actor = { unit: { generation: 3 }, actions: mailbox(), fire: mailbox() }, calls = [];
  const world = Object.fromEntries(['actions', 'fire'].map(lane => [lane, (client, message) => {
    callback?.(lane, message);
    const accepted = message.id % 5 !== 0;
    calls.push({ client, lane, message: structuredClone(message), accepted });
    if (accepted) { actor[lane].received = message.id; actor[lane].outcomes.push({ id: message.id, status: ['consumed', 'expired', 'cleared'][message.id % 3] }); }
    return accepted;
  }]));
  const edges = new EntityEdges({ bridge, actor, world, client: 'client', registry: { classes: new Map([[1, 'unit']]), posts: new Map([[2, 'post']]) } });
  return { actor, calls, edges };
}
const plainQueue = e => [...e.receiver.queue].map(([id, value]) => [id, { ...value }]);
function same(a, b, at) {
  assert.deepEqual(a.calls, b.calls, `${at}: deliveries`);
  assert.deepEqual(a.edges.feedback(), b.edges.feedback(), `${at}: feedback`);
  assert.deepEqual([...a.edges.pending], [...b.edges.pending], `${at}: pending`);
  assert.deepEqual([...a.edges.results], [...b.edges.results], `${at}: results`);
  assert.deepEqual(a.edges.dropped, b.edges.dropped, `${at}: dropped`);
  assert.deepEqual(plainQueue(a.edges), plainQueue(b.edges), `${at}: receiver queue`);
  assert.deepEqual([...a.edges.receiver.seen.keys()], [...b.edges.receiver.seen.keys()], `${at}: seen order`);
  assert.equal(a.edges.actionSerial, b.edges.actionSerial); assert.equal(a.edges.fireSerial, b.edges.fireSerial);
}
function edge(id, tick) {
  return { epoch: 1, generation: 3, id, tick, ...[
    { kind: 'action', action: INPUT_ACTIONS[id % INPUT_ACTIONS.length] },
    { kind: 'fire', button: id % 2 ? 'firePrimary' : 'fireSecondary', pressed: !!(id % 3), viewTick: tick - 0.25, aimYaw: -0, aimPitch: 0.2 },
    { kind: 'spawn', classId: id % 4 === 0 ? 99 : 1, postId: id % 3 === 0 ? 88 : 2 },
    { kind: 'team', team: id % 7 + 1 }, { kind: 'suicide' }, { kind: 'cancel' }, { kind: 'unknown' },
  ][id % 7] };
}

test('native reliable edges match reordered receipt, delivery and outcome traces', async () => {
  const native = fixture(bridgeFor(await loadSim())), js = fixture();
  let highest = 0;
  for (let tick = 1; tick <= 210; tick++) {
    const packet = { epoch: 1, generation: 3, edges: [] };
    for (let i = 0; i < tick % 36; i++) packet.edges.push(edge(++highest, tick + (i % 9 === 0 ? 140 : i % 11 === 0 ? -40 : i % 3 - 1)));
    if (tick % 5 === 0) packet.edges.reverse();
    if (tick % 8 === 0) packet.edges.push(edge(highest - 2, tick), edge(1, tick));
    for (const f of [native, js]) {
      f.edges.receive(structuredClone(packet), tick);
      f.edges.receive(structuredClone(packet), tick);
      f.edges.advance(tick + 1);
      if (tick % 3 !== 0) f.edges.consumed(tick + 1);
      if (tick % 7 === 0) f.edges.acknowledge({ epoch: 1, generation: 3, ids: f.edges.feedback().results.slice(0, 11).map(r => r.id) });
      if (tick % 29 === 0) f.edges.receive({ epoch: 2, generation: 3, edges: [edge(9999, tick)] }, tick + 2);
      if (tick === 81) f.edges.capacity = 64;
      if (tick === 123) f.edges.reset(1, 3);
    }
    same(native, js, tick);
  }
  native.edges.dispose();
  assert.throws(() => native.edges.feedback(), /disposed/);
});

test('native edge delivery tolerates shared-bridge calls and memory growth inside callbacks', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  const registry = { inputCodes: INPUT_CODES, bindingProfiles: new Map([[0, {}]]) };
  let entered = false;
  const native = fixture(bridge, () => {
    assert.equal(Buffer.from(encodeEntityInput({ epoch: 1, generation: 1 }, registry)).toString('hex'), '424e4931010000000100000000000000');
    if (!entered) { entered = true; instance.exports.memory.grow(1); }
  }), js = fixture();
  for (const f of [native, js]) {
    f.edges.receive({ epoch: 1, generation: 3, edges: [edge(1, 5), edge(7, 4), edge(4, 5)] }, 5);
    f.edges.advance(6); f.edges.consumed(6);
  }
  same(native, js, 'reentrant'); native.edges.dispose();
});

const inputGolden = JSON.parse(readFileSync(new URL('./fixtures/input-encode-golden.json', import.meta.url)));
let inputCase = 0;
function compareEncode(packet, registry) {
  const expected = inputGolden[inputCase++];
  if (expected.error) assert.throws(() => encodeEntityInput(packet, registry), e => e.message === expected.error, expected.error);
  else assert.equal(Buffer.from(encodeEntityInput(packet, registry)).toString('hex'), expected.hex);
}
test('native input bytes and validation match contexts, profiles, held bits and every edge', async () => {
  const registry = { inputCodes: INPUT_CODES, bindingProfiles: new Map([[0, {}], [3, { brake: ['KeyS'], throttle: ['KeyW'] }]]) };
  for (let i = 1; i <= 480; i++) {
    const commands = Array.from({ length: i % 9 }, (_, j) => ({ sequence: j + i, tick: 10 + i, down: [INPUT_CODES[(i + j) % INPUT_CODES.length], 'KeyW'],
      lookTotal: [i / 39, -j / 71], mouseScale: i % 3 ? 1 : undefined, aspect: 16 / 9, viewTick: i + j / 5,
      firstPerson: !!(i % 2), invertMouse: false,
      flightSettings: i % 4 ? { bindings: i % 3 ? {} : { throttle: ['KeyW'], brake: ['KeyS'] }, sensitivity: 1.2, curve: 0.9, invert: !!j } : undefined,
      gamepad: i % 3 ? { axes: [j / 8, -0, 0.7, -0.1], buttons: [{ pressed: true }, { pressed: false }, { pressed: !!j }] } : undefined }));
    const edges = Array.from({ length: i % 17 }, (_, j) => edge(i + j, 10 + i)).filter(e => e.kind !== 'unknown').map(e => ({ ...e, generation: 3 }));
    compareEncode({ epoch: 1, generation: 3, commands, edges, snapshotAck: i % 2 ? { epoch: 1, tick: i } : null }, registry);
  }
  const base = { epoch: 1, generation: 3, commands: [{ sequence: 1, tick: 5, down: [], lookTotal: [0, 0] }] };
  for (const bad of [-1, 1.5, NaN, Infinity, undefined, null, '1', 0x100000000]) compareEncode({ ...base, epoch: bad }, registry);
  for (const key of ['mouseScale', 'aspect', 'viewTick']) for (const bad of [-1, 0, 10000000000, NaN, null, '1']) {
    compareEncode({ ...base, commands: [{ ...base.commands[0], [key]: bad }] }, registry);
  }
  for (const bad of [-1, 0, 8, NaN, null, '1']) compareEncode({ ...base, edges: [{ id: 1, tick: 5, kind: 'team', team: bad }] }, registry);
  for (const bad of [-1, 65536, 1.5, NaN]) compareEncode({ ...base, edges: [{ id: 1, tick: 5, kind: 'spawn', classId: bad }] }, registry);
  compareEncode({ ...base, commands: [{ ...base.commands[0], flightSettings: { bindings: { mystery: ['KeyA'] } } }] }, registry);
  compareEncode({ ...base, commands: [{ ...base.commands[0], down: ['unknown'] }] }, registry);
  compareEncode({ ...base, commands: [{ ...base.commands[0], epoch: 9 }] }, registry);
  compareEncode({ ...base, edges: [{ id: 0, tick: 5, kind: 'cancel' }] }, registry);
  assert.equal(inputCase, inputGolden.length);
  for (const ack of [{ epoch: 1, tick: 0 }, { epoch: 65535, tick: 0xffffffff }]) {
    assert.deepEqual(encodeSnapshotAckKeepalive(ack, registry), encodeSnapshotAckKeepalive(ack));
  }
});
