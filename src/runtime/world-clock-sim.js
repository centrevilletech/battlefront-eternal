// The host supplies wall time and invokes the tick. Native code retains
// the clock's exact arithmetic and ordering across those callbacks.
import { putWorldClockInput, WorldClockResultMirror, syncWorldClockResult } from '../sim/gen/world.js';

const fields = ['hz', 'dt', 'maxSteps', 'budget', 'maxDebt', 'tick', 'elapsed', 'compensation', 'debt', 'alpha', 'overloads', 'dropped', 'stepCost'];
const read = source => Object.fromEntries(fields.map(key => [key, source[key]]));
const write = (source, value) => { for (const key of fields) source[key] = value[key]; };
const now = () => globalThis.performance ? performance.now() : Date.now();

export function clockPorts(bridge) {
  const x = bridge.x, contexts = [], owned = new WeakSet();
  const input = { words: x.world_clock_words(0), put: putWorldClockInput };
  const output = { Mirror: WorldClockResultMirror, sync: syncWorldClockResult };
  bridge.register({ worldClock(args) {
    const context = contexts.at(-1), op = args.op, dt = args.dt, tick = args.tick;
    // Publish before the host runs, then take any callback's edits back.
    // The generated args record is reused by nested imports.
    write(context.source, args.state);
    let time = 0;
    if (op === 'now') time = now();
    else { const step = context.step; step(dt, tick); }
    return { state: read(context.source), time };
  } });
  function clock(source) {
    if (owned.has(source)) return source;
    function run(op, seconds, step = null, limited = true) {
      contexts.push({ source, step });
      try {
        bridge.callArgs(input, { op, state: read(source), seconds: typeof seconds === 'number' ? seconds : NaN, limited: Boolean(limited) });
        const out = bridge.callResult(output, x.world_clock_run());
        write(source, out.state);
        if (out.error) throw Error(out.error);
        return out.ran;
      } finally { contexts.pop(); }
    }
    Object.defineProperties(source, {
      advance: { configurable: true, value: (seconds, step, limited = true) => run('advance', seconds, step, limited) },
      reset: { configurable: true, value() { run('reset'); } },
      rebase: { configurable: true, value() { run('rebase'); } },
      time: { configurable: true, get: () => x.world_clock_time(source.tick, source.hz) },
    });
    owned.add(source); return source;
  }
  return { clock, ownsClock: source => owned.has(source) };
}
