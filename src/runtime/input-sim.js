// Reliable actions use the world's existing module. JavaScript checks
// incoming value shapes; native code owns stamps, bounds and consumption.
import { bridgeFor } from '../sim/runtime.js';
import { putIaInput, IaResultMirror, syncIaResult } from '../sim/gen/input.js';

const number = value => typeof value === 'number' ? value : NaN;
const humanValue = value => ({ kind: value.kind, team: value.team ?? 0, className: value.className ?? null, post: value.post ?? null });
const humanCopy = value => value.kind === 'team' ? { kind: 'team', team: value.team }
  : value.kind === 'spawn' ? { kind: 'spawn', className: value.className, post: value.post }
  : { kind: value.kind };
function humanInput(value) {
  const empty = { command: { kind: 'cancel', team: 0, className: null, post: null }, valid: false };
  if (!value || typeof value !== 'object') return empty;
  if (value.kind === 'team') return { command: { ...empty.command, kind: 'team', team: number(value.team) }, valid: true };
  if (value.kind === 'spawn') {
    const post = value.post === undefined ? null : value.post;
    return { command: { ...empty.command, kind: 'spawn', className: typeof value.className === 'string' ? value.className : null,
      post: typeof post === 'string' ? post : null }, valid: post === null || typeof post === 'string' };
  }
  if (value.kind === 'suicide' || value.kind === 'cancel') return { command: { ...empty.command, kind: value.kind }, valid: true };
  return empty;
}

export function createInputSim(instance) {
  const bridge = bridgeFor(instance), x = bridge.x, owned = new WeakSet();
  const input = { words: x.input_actions_words(0), put: putIaInput };
  const output = { Mirror: IaResultMirror, sync: syncIaResult };
  function actions(source) {
    if (owned.has(source)) return source;
    function run(op, args = {}) {
      const state = { ...source, queue: source.queue.map(e => ({ ...e, human: e.human.map(humanValue) })) };
      bridge.callArgs(input, { op, state, entry: null, sameEpoch: false, sameGeneration: false, payloadValid: false,
        humanValid: true, human: [], tick: 0, generation: 0, id: 0, status: null, ...args });
      const out = bridge.callResult(output, x.input_actions_run()), next = out.state;
      for (const key of ['epoch', 'capacity', 'received', 'ack', 'discarded']) source[key] = next[key];
      const old = new Map(source.queue.map(e => [e.id, e]));
      source.queue.splice(0, source.queue.length, ...next.queue.map(e => {
        const entry = old.get(e.id) ?? { actions: [], human: [] };
        entry.id = e.id; entry.tick = e.tick; entry.generation = e.generation;
        entry.actions.splice(0, entry.actions.length, ...e.actions);
        // take() replaces the entry's human array after returning its choices.
        entry.human = e.human.map(humanCopy);
        return entry;
      }));
      if (next.outcomes === null) source.outcomes = null;
      else {
        source.outcomes ??= [];
        source.outcomes.splice(0, source.outcomes.length, ...next.outcomes.map(e => ({ id: e.id, status: e.status })));
      }
      if (out.error) throw Error(out.error);
      return { accepted: out.accepted, actions: [...out.actions], human: out.human.map(humanCopy),
        outcomes: out.outcomes.map(e => ({ id: e.id, status: e.status })) };
    }
    Object.defineProperties(source, Object.fromEntries(Object.entries({
      push(message, tick, generation) {
        const list = message?.actions;
        const payloadValid = Array.isArray(list) && list.every(a => typeof a === 'string');
        const humans = message?.human;
        return run('push', { tick, generation, sameEpoch: message?.epoch === source.epoch, sameGeneration: message?.generation === generation,
          payloadValid, humanValid: humans === undefined || Array.isArray(humans), human: Array.isArray(humans) ? humans.map(humanInput) : [],
          entry: message ? { id: number(message.id), tick: number(message.tick), generation,
            actions: payloadValid ? list : [], human: [] } : null }).accepted;
      },
      take(tick, generation) { const { actions, human } = run('take', { tick, generation }); return { actions, human }; },
      clear() { run('clear'); },
      enableOutcomes() { run('enable'); return source; },
      drainOutcomes() { return run('drain').outcomes; },
      outcome(id, status) { run('outcome', { id, status }); },
    }).map(([key, value]) => [key, { value, configurable: true }])));
    owned.add(source);
    return source;
  }
  return { bridge, actions, owns: source => owned.has(source) };
}
