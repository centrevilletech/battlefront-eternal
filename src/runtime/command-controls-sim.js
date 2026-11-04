// The source objects remain the public facade while Rust consumes their
// ordered edges and stamps. Tokens preserve arbitrary debug key identity.
import { HOLDS, WORLD_AXES } from '../input.js';
import { putIqInput, IqResultMirror, syncIqResult, putIcInput, IcResultMirror, syncIcResult, putIkInput } from '../sim/gen/input.js';

function tokens() {
  const values = [null, undefined, NaN, 0, -0], ids = new Map([[null, 0], [undefined, 1], [NaN, 2], [0, 3]]);
  const id = value => {
    if (Object.is(value, -0)) return 4;
    let n = ids.get(value);
    if (n === undefined) { n = values.length; values.push(value); ids.set(value, n); }
    return n;
  };
  return { values, id };
}
const indexKey = key => /^(0|[1-9][0-9]*)$/.test(key);
function queueState(q, token) {
  return { capacity: q.capacity, read: q.read, write: q.write, length: q.length, itemsLength: q.items.length,
    items: Object.keys(q.items).filter(key => indexKey(key) && Number(key) < q.items.length).map(key => ({ index: Number(key), value: token.id(q.items[key]) })),
    counts: [...q.counts].map(([name, count]) => ({ name: token.id(name), count })),
  };
}
function syncQueue(q, next, replaced, token) {
  for (const key of ['capacity', 'read', 'write', 'length']) q[key] = next[key];
  if (replaced) q.items = new Array(next.itemsLength);
  else {
    q.items.length = next.itemsLength;
    const kept = new Set(next.items.map(item => String(item.index)));
    for (const key of Object.keys(q.items)) if (indexKey(key) && !kept.has(key)) delete q.items[key];
  }
  for (const item of next.items) q.items[item.index] = token.values[item.value];
  const counts = new Map(next.counts.map(entry => [token.values[entry.name], entry.count]));
  for (const name of q.counts.keys()) if (!counts.has(name)) q.counts.delete(name);
  for (const [name, count] of counts) q.counts.set(name, count);
}
function methods(source, methods) {
  Object.defineProperties(source, Object.fromEntries(Object.entries(methods).map(([key, value]) => [key, { value, configurable: true }])));
}

export function commandControlPorts(bridge, owned) {
  const x = bridge.x;
  const queueInput = { words: x.input_queue_words(0), put: putIqInput };
  const queueOutput = { Mirror: IqResultMirror, sync: syncIqResult };
  const controlInput = { words: x.input_controls_words(0), put: putIcInput };
  const controlOutput = { Mirror: IcResultMirror, sync: syncIcResult };
  const keysInput = { words: x.input_controls_words(2), put: putIkInput };

  function actionQueue(source) {
    if (owned.has(source)) return source;
    function run(op, name) {
      const token = tokens();
      bridge.callArgs(queueInput, { op, state: queueState(source, token), name: token.id(name) });
      const out = bridge.callResult(queueOutput, x.input_queue_run());
      syncQueue(source, out.state, out.replacedItems, token);
      if (out.error) throw Error(out.error);
      return out.accepted;
    }
    methods(source, {
      add(name) { run('add', name); return source; },
      delete(name) { return run('delete', name); },
      has(name) { return run('has', name); },
      clear() { run('clear'); },
    });
    owned.add(source); return source;
  }

  function controls(source) {
    if (owned.has(source)) return source;
    actionQueue(source.queued);
    function run(op, command = {}) {
      const token = tokens();
      bridge.callArgs(controlInput, { op, state: { epoch: source.epoch, generation: source.generation, sequence: source.sequence,
        down: Array.from(source.down, token.id), queue: queueState(source.queued, token) },
      epoch: command.epoch, generation: command.generation, sequence: command.sequence });
      const out = bridge.callResult(controlOutput, x.input_controls_run());
      for (const key of ['epoch', 'generation', 'sequence']) source[key] = out.state[key];
      if (out.accepted || op === 'clear') {
        source.down.clear();
        for (const name of out.state.down) source.down.add(token.values[name]);
      }
      syncQueue(source.queued, out.state.queue, out.replacedItems, token);
      return out.accepted;
    }
    function axis(plus, minus = []) {
      const token = tokens();
      bridge.callArgs(keysInput, { down: Array.from(source.down, token.id), plus: plus.filter(() => true).map(token.id), minus: minus.filter(() => true).map(token.id) });
      return x.input_controls_axis();
    }
    methods(source, {
      accept(command) {
        if (!run('accept', command)) return false;
        // Iterate only after admission and stamp updates. Iterators and
        // overflow may throw with a partly consumed command in the source.
        for (const key of command.down) source.down.add(key);
        for (const action of command.actions) source.queued.add(action);
        return true;
      },
      take(name) { return source.queued.delete(name); },
      held(name) { const codes = source.flightScheme?.()?.[name] || HOLDS[name] || []; return Boolean(axis(codes)); },
      axis(name) { const [plus, minus] = WORLD_AXES[name] || [[], []]; return axis(plus, minus); },
      clear() { run('clear'); },
    });
    owned.add(source); return source;
  }
  return { actionQueue, controls };
}
