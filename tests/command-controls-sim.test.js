import test from 'node:test';
import assert from 'node:assert/strict';
import { ActionQueue } from '../src/runtime/action-queue.js';
import { CommandControls } from '../src/runtime/commands.js';
import { createInputSim } from '../src/runtime/input-sim.js';
import { loadSim } from '../src/sim/module.js';
import { ClassicRandom } from '../src/ai/random.js';

const queueState = q => ({ capacity: q.capacity, items: q.items.slice(), counts: [...q.counts], read: q.read, write: q.write, length: q.length });
const controlState = c => ({ epoch: c.epoch, generation: c.generation, sequence: c.sequence, down: [...c.down], queued: queueState(c.queued) });
const invoke = (source, method, args) => {
  try { const value = source[method](...args); return { value: value === source ? 'self' : value }; }
  catch (error) { return { error: error.message }; }
};

test('native action queues preserve ring compaction, repeated edges, sparse storage and key identity', async () => {
  const sim = createInputSim(await loadSim()), object = { key: 1 }, symbol = Symbol('key');
  const names = ['jump', 'use', 'reload', undefined, null, NaN, 0, -0, 1, Infinity, object, symbol];
  let operations = 0;
  for (const seed of [1, 7, 79, 911, 4321]) {
    const random = new ClassicRandom(seed), pick = values => values[Math.floor(random.uniform() * values.length)];
    const js = new ActionQueue(7), native = sim.actionQueue(new ActionQueue(7));
    for (let i = 0; i < 1600; i++) {
      const method = pick(['add', 'add', 'delete', 'delete', 'has', 'clear']);
      const args = [pick(names)], old = native.items, oldJs = js.items;
      assert.deepEqual(invoke(native, method, args), invoke(js, method, args), `seed ${seed}, ${i}, ${method}`);
      assert.deepEqual(queueState(native), queueState(js), `state ${seed}, ${i}, ${method}`);
      assert.equal(native.items === old, js.items === oldJs, 'compaction alone replaces the backing array');
      assert.deepEqual([...native], [...js]);
      operations++;
    }
  }
  assert.equal(operations, 8000);
});

test('native command controls twin epochs, lives, repeated presses and dynamic seat bindings', async () => {
  const sim = createInputSim(await loadSim());
  let operations = 0;
  for (const seed of [1, 7, 79, 911, 4321]) {
    const random = new ClassicRandom(seed), pick = values => values[Math.floor(random.uniform() * values.length)];
    const js = new CommandControls(), native = new CommandControls();
    js.queued = new ActionQueue(9); native.queued = new ActionQueue(9); sim.controls(native);
    let sequence = 0, generation = 1, epoch = 1, alternate = false;
    const scheme = () => alternate ? { firePrimary: ['ArrowUp'], fireSecondary: [], boost: ['Space'] } : null;
    js.flightScheme = native.flightScheme = scheme;
    for (let i = 0; i < 1600; i++) {
      let method = pick(['accept', 'accept', 'accept', 'clear', 'take', 'held', 'axis']), args;
      if (method === 'accept') {
        if (i % 37 === 0) generation++;
        if (i % 191 === 0) epoch++;
        const command = { epoch, generation, sequence: ++sequence,
          down: pick([[], ['KeyW', 'ArrowUp', 'Mouse0'], ['ArrowDown', 'KeyA', 'AltLeft'], ['Space', 'NumpadMultiply'], ['KeyD', 'ArrowLeft']]),
          actions: pick([[], ['jump'], ['jump', 'jump', 'use'], ['reload', 'use', 'reload']]) };
        if (i % 3 === 0) Object.assign(command, pick([{ sequence: sequence - 1 }, { generation: generation - 1 }, { epoch: epoch - 1 }, { epoch: NaN }, { epoch: undefined, generation: undefined, sequence: undefined }]));
        args = [command];
      } else if (method === 'take') args = [pick(['jump', 'reload', 'use', 'missing'])];
      else if (method === 'held') args = [pick(['firePrimary', 'fireSecondary', 'boost', 'missing'])];
      else if (method === 'axis') args = [pick(['forward', 'right', 'turn', 'missing'])];
      else args = [];
      alternate = i % 11 < 5;
      assert.deepEqual(invoke(native, method, args), invoke(js, method, args), `seed ${seed}, ${i}, ${method}`);
      assert.deepEqual(controlState(native), controlState(js), `state ${seed}, ${i}, ${method}`);
      for (const name of ['forward', 'right', 'turn']) assert.equal(native.axis(name), js.axis(name));
      operations++;
    }
  }
  assert.equal(operations, 8000);
});

test('native controls retain partial command effects, public collections and reentrant iterators', async () => {
  const instance = await loadSim(), sim = createInputSim(instance);
  const js = new CommandControls(), native = new CommandControls();
  js.queued = new ActionQueue(2); native.queued = new ActionQueue(2); sim.controls(native);
  const down = native.down, queue = native.queued, counts = queue.counts;
  const cmd = (sequence, actions = [], keys = []) => ({ epoch: 1, generation: 1, sequence, down: keys, actions });
  for (const command of [cmd(1, ['jump', 'jump', 'reload'], ['KeyW']), cmd(1, null, null), cmd(2, null, ['Mouse0']), cmd(3, [], null)]) {
    assert.deepEqual(invoke(native, 'accept', [command]), invoke(js, 'accept', [command]));
    assert.deepEqual(controlState(native), controlState(js));
  }
  assert.equal(native.down, down); assert.equal(native.queued, queue); assert.equal(queue.counts, counts);
  native.down.add('ArrowRight'); js.down.add('ArrowRight');
  native.queued.clear(); js.queued.clear();
  native.queued.add('use'); js.queued.add('use');
  native.queued.items[0] = js.queued.items[0] = 'reload';
  native.queued.counts.set('reload', 1); js.queued.counts.set('reload', 1);
  assert.deepEqual(invoke(native, 'take', ['reload']), invoke(js, 'take', ['reload']));
  assert.deepEqual(controlState(native), controlState(js));
  function reentrant(c) {
    return cmd(4, { *[Symbol.iterator]() { yield 'jump'; c.clear(); yield 'use'; } }, {
      *[Symbol.iterator]() { yield 'Mouse0'; c.down.add('KeyA'); yield 'ArrowUp'; },
    });
  }
  assert.equal(native.accept(reentrant(native)), js.accept(reentrant(js)));
  assert.deepEqual(controlState(native), controlState(js));
  let calls = 0;
  native.flightScheme = () => { calls++; sim.controls(new CommandControls()).accept(cmd(1, [], ['KeyS'])); native.down.add('Space'); native.down.add('KeyW'); native.down.add('KeyA'); return { boost: ['Space'] }; };
  instance.exports.memory.grow(1);
  assert.equal(native.held('boost'), true); assert.equal(calls, 1);
  assert.equal(native.axis('forward'), 1); assert.equal(native.axis('right'), -1);
  assert.equal(sim.controls(native), native); assert.equal(sim.owns(queue), true);
  native.flightScheme = () => ({ boost: new Array(2) }); native.down.add(undefined);
  assert.equal(native.held('boost'), false, 'binding holes are skipped by some');
});
