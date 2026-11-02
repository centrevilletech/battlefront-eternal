import test from 'node:test';
import assert from 'node:assert/strict';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { createWorldPump } from '../src/runtime/local-world-pump.js';

// A worker's event loop on a fake clock: queued tasks first, then the timer.
function harness(stepMs) {
  let t = 1000, timer = null, nextId = 0;
  const tasks = [], starts = [];
  const clock = new FixedClock({ maxSteps: 4, budget: .05, maxDebt: 8 });
  const pump = createWorldPump(clock, (_, tick) => { starts.push([tick, t]); t += stepMs(tick); }, {
    now: () => t,
    setTimer: (fn, ms) => { assert(Number.isInteger(ms) && ms > 0, `a whole, positive timeout (${ms})`); timer = { fn, at: t + ms, id: ++nextId }; return timer.id; },
    clearTimer: id => { if (timer?.id === id) timer = null; },
    soon: fn => tasks.push(fn),
  });
  const run = until => {
    while (t < until) {
      if (tasks.length) { tasks.shift()(); continue; }
      if (!timer) break;
      const { fn, at } = timer; timer = null; t = Math.max(t, at); fn();
    }
  };
  return { clock, pump, starts, run, tasks, get t() { return t; }, set t(v) { t = v; } };
}

test('a tick starts when it is due, not a step later, and a long step\'s successor starts at once', () => {
  const tickMs = 1000 / 60;
  // Steps of 10 ms, and one of 27 ms (tick 30).
  const h = harness(tick => tick === 30 ? 27 : 10);
  h.pump.start(); h.run(1000 + 1000);
  const late = h.starts.map(([tick, at]) => at - 1000 - tick * tickMs);
  // Each tick is due at tick/60 s; a timeout of whole milliseconds, rounded
  // up, wakes it at most a millisecond late and never early.
  for (const [i, [tick]] of h.starts.entries()) if (tick !== 31 && tick !== 32) assert(late[i] >= -1e-9 && late[i] <= 1 + 1e-9, `tick ${tick} ${late[i].toFixed(2)} ms late`);
  // Tick 31 was due during tick 30's 27 ms: it starts as tick 30 ends.
  const [, at30] = h.starts.find(([tick]) => tick === 30), [, at31] = h.starts.find(([tick]) => tick === 31);
  assert.equal(at31, at30 + 27);
  assert.deepEqual(h.starts.map(([tick]) => tick), Array.from({ length: h.starts.length }, (_, i) => i + 1));
  assert.equal(h.clock.dropped, 0);
});

test('a world behind runs its due ticks back to back, without waiting on a timer', () => {
  const h = harness(() => 20);
  h.pump.start(); h.run(1000 + 500);
  // 20 ms steps on a 16.7 ms tick: always behind, so no gap between ticks.
  for (let i = 1; i < h.starts.length; i++) assert.equal(h.starts[i][1] - h.starts[i - 1][1], 20);
});

test('stopping cancels the pending wake, and starting again runs one heartbeat', () => {
  const h = harness(() => 30);
  h.pump.start(); h.run(1000 + 100);
  assert(h.tasks.length > 0, 'behind: the next wake is queued');
  // Suspended and resumed before the queued wake ran: it is stale, and the
  // resumed pump is the only one stepping.
  h.pump.stop(); h.clock.rebase(); h.pump.start();
  const resumed = h.starts.length;
  h.run(h.t + 300);
  assert(h.starts.length > resumed + 5);
  for (let i = resumed + 1; i < h.starts.length; i++) assert.equal(h.starts[i][1] - h.starts[i - 1][1], 30, 'one step at a time');
  assert.deepEqual(h.starts.map(([tick]) => tick), Array.from({ length: h.starts.length }, (_, i) => i + 1));
  h.pump.stop();
  const ran = h.starts.length;
  h.run(h.t + 300);
  assert.equal(h.starts.length, ran, 'a stopped pump runs nothing');
});
