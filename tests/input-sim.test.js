import test from 'node:test';
import assert from 'node:assert/strict';
import { InputActions } from '../src/runtime/input-actions.js';
import { createInputSim } from '../src/runtime/input-sim.js';
import { loadSim } from '../src/sim/module.js';
import { ClassicRandom } from '../src/ai/random.js';

const packet = (id, tick, generation, extra = {}) => ({ epoch: 1, id, tick, generation, actions: ['jump', 'jump', 'reload'], ...extra });
const snapshot = source => structuredClone({ ...source });
const invoke = (source, method, args) => {
  try { const value = source[method](...args); return { value: value === source ? 'self' : structuredClone(value) }; }
  catch (error) { return { error: error.message }; }
};

test('native reliable actions match source ordering, validation, bounds and partial failures', async () => {
  const input = createInputSim(await loadSim());
  let operations = 0;
  for (const seed of [1, 7, 79, 911, 4321]) {
    const random = new ClassicRandom(seed), pick = values => values[Math.floor(random.uniform() * values.length)];
    const js = new InputActions(1, 4), native = input.actions(new InputActions(1, 4));
    assert.equal(input.owns(native), true);
    let tick = 100, id = 0, generation = 2;
    for (let i = 0; i < 1600; i++) {
      let method, args;
      const op = pick(['push', 'push', 'take', 'take', 'clear', 'enableOutcomes', 'drainOutcomes', 'outcome']);
      if (op === 'push') {
        const message = packet(++id, tick + pick([-121, -120, -2, 0, 1, 120, 121]), generation);
        if (i % 3 === 0) Object.assign(message, pick([
          { epoch: 2 }, { id: NaN }, { id: '1' }, { id: 1 }, { tick: 0 }, { tick: 1.25 }, { generation: generation - 1 },
          { actions: null }, { actions: [false] }, { actions: ['firePrimary'] }, { actions: ['fireSecondary'] },
          { actions: ['😀'.repeat(33)] }, { actions: Array(65).fill('jump') },
          { actions: [] }, { actions: ['reload', 'use', 'reload', 'use', 'reload'] },
          { human: null }, { human: [] }, { human: [{ kind: 'other' }] }, { human: [{ kind: 'team', team: 8 }] },
          { human: [{ kind: 'team', team: '2' }] }, { human: [{ kind: 'team', team: 2 }] },
          { human: [{ kind: 'spawn', className: '😀'.repeat(32) }] },
          { human: [{ kind: 'spawn', className: '😀'.repeat(33) }] },
          { human: [{ kind: 'spawn', className: 'trooper', post: false }] },
          { human: [{ kind: 'spawn', className: 'trooper', post: 'cp1' }, { kind: 'suicide' }, { kind: 'cancel' }] },
          { human: Array(17).fill({ kind: 'cancel' }) },
        ]));
        method = 'push'; args = [message, tick, generation];
      } else if (op === 'take') {
        tick += pick([0, 1, 1, 2, 120, 121]); if (i % 79 === 0) generation++;
        method = 'take'; args = [tick, generation];
      } else if (op === 'outcome') { method = op; args = [id, 'manual']; }
      else { method = op; args = []; }
      const expected = invoke(js, method, args), actual = invoke(native, method, args);
      assert.deepEqual(actual, expected, `seed ${seed}, step ${i}, ${method}`);
      assert.deepEqual(snapshot(native), snapshot(js), `state seed ${seed}, step ${i}, ${method}`);
      operations++;
    }
  }
  assert.equal(operations, 8000);
});

test('native action results survive later calls and separate queues share one instance safely', async () => {
  const instance = await loadSim(), input = createInputSim(instance);
  const first = input.actions(new InputActions()), second = input.actions(new InputActions());
  const queue = first.queue;
  first.push(packet(1, 1, 1, { human: [{ kind: 'spawn', className: 'trooper', post: null }] }), 1, 1);
  const entry = first.queue[0], actions = entry.actions;
  const value = first.take(1, 1);
  assert.equal(first.queue, queue); assert.equal(first.queue[0], entry); assert.equal(entry.actions, actions);
  instance.exports.memory.grow(1);
  second.push(packet(2, 2, 2), 2, 2); second.take(2, 2); first.take(2, 1);
  assert.deepEqual(value, { actions: ['jump', 'reload'], human: [{ kind: 'spawn', className: 'trooper', post: null }] });
  assert.equal(first.ack, 1); assert.equal(second.ack, 0);
  assert.equal(input.actions(first), first);
});

test('native action outcome overflow preserves the exact partially consumed state', async () => {
  const input = createInputSim(await loadSim()), js = new InputActions(1, 1).enableOutcomes();
  const native = input.actions(new InputActions(1, 1)).enableOutcomes();
  for (const source of [js, native]) {
    source.outcome(1, 'manual'); source.outcome(2, 'manual');
    source.push(packet(1, 1, 1, { actions: ['jump'], human: [{ kind: 'suicide' }] }), 1, 1);
  }
  assert.deepEqual(invoke(native, 'take', [1, 1]), invoke(js, 'take', [1, 1]));
  assert.deepEqual(snapshot(native), snapshot(js));
  assert.deepEqual(native.queue[0].actions, []);
  assert.deepEqual(native.queue[0].human, []);
  native.drainOutcomes(); js.drainOutcomes();
  assert.deepEqual(native.take(2, 1), js.take(2, 1));
  assert.deepEqual(snapshot(native), snapshot(js));
});
