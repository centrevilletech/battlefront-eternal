import test from 'node:test';
import assert from 'node:assert/strict';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { createWorldSim } from '../src/runtime/world-sim.js';
import { loadSim } from '../src/sim/module.js';

const state = clock => ({ ...clock, time: clock.time });
function invoke(clock, seconds, callback, limited) {
  try { return { value: clock.advance(seconds, callback, limited) }; }
  catch (error) { return { error: error.message }; }
}
function fakeTime(run) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'performance');
  let time = 0, reads = 0;
  Object.defineProperty(globalThis, 'performance', { configurable: true, value: { now() { reads++; return time; } } });
  try { return run({ add(value) { time += value; }, reads: () => reads }); }
  finally { Object.defineProperty(globalThis, 'performance', descriptor); }
}

test('native fixed clocks preserve compensated tick traces across rates, hitches, resets and rebasing', async () => {
  const world = createWorldSim(await loadSim());
  for (const hz of [30, 60, 144]) for (const schedule of [[1 / 30], [1 / 60], [1 / 144], [1 / 240], [1 / 120, 0.075, 0, 1 / 240]]) {
    const js = new FixedClock({ hz, maxSteps: 4 }), native = world.clock(new FixedClock({ hz, maxSteps: 4 }));
    for (let frame = 0; frame < 240; frame++) {
      if (frame === 90) { js.rebase(); native.rebase(); }
      if (frame === 170) { js.reset(); native.reset(); }
      const expected = [], actual = [], seconds = schedule[frame % schedule.length], limited = frame % 3 !== 0;
      const a = invoke(js, seconds, (dt, tick) => expected.push([dt, tick, state(js)]), limited);
      const b = invoke(native, seconds, (dt, tick) => actual.push([dt, tick, state(native)]), limited);
      assert.deepEqual(b, a); assert.deepEqual(actual, expected); assert.deepEqual(state(native), state(js));
    }
    assert.equal(world.ownsClock(native), true); assert.equal(world.clock(native), native);
  }
});

test('native fixed clocks preserve wall-clock budgets, smoothing, debt caps and callback-visible state', async () => {
  const world = createWorldSim(await loadSim());
  for (const options of [{ budget: 0.035, maxDebt: 8 }, { budget: 0.002, maxDebt: 0 }, { budget: 0.15, maxDebt: Infinity, maxSteps: 3 }]) {
    const run = native => fakeTime(time => {
      const source = new FixedClock(options), clock = native ? world.clock(source) : source;
      const trace = [];
      for (let frame = 0; frame < 90; frame++) {
        const steps = [];
        const result = invoke(clock, [0.1, 1 / 60, 0, 0.25][frame % 4], (dt, tick) => {
          steps.push([dt, tick, state(clock)]);
          time.add([0, 2, 18, 50, 81.125][tick % 5]);
        }, frame % 7 !== 0);
        trace.push([result, steps, state(clock), time.reads()]);
      }
      return trace;
    });
    assert.deepEqual(run(true), run(false));
  }
});

test('native clock callbacks can reset, reenter another clock and grow memory without losing state', async () => {
  const instance = await loadSim(), world = createWorldSim(instance);
  const run = native => {
    const a = new FixedClock({ maxSteps: 2 }), b = new FixedClock({ hz: 30 });
    if (native) { world.clock(a); world.clock(b); }
    const trace = [];
    let nested = false;
    const result = invoke(a, 0.05, (dt, tick) => {
      trace.push(['outer', dt, tick, state(a)]);
      b.advance(1 / 30, (h, n) => trace.push(['other', h, n, state(b)]));
      if (!nested) {
        nested = true;
        a.advance(0, (h, n) => trace.push(['nested', h, n, state(a)]));
        a.reset(); a.hz = 120; a.dt = 1 / 120;
        if (native) instance.exports.memory.grow(1);
      }
    }, false);
    return { trace, result, a: state(a), b: state(b) };
  };
  assert.deepEqual(run(true), run(false));
  assert.equal(createWorldSim(instance), world, 'one callback dispatcher per shared bridge');
});

test('native fixed clocks preserve failed-tick partial state and resume after host exceptions', async () => {
  const world = createWorldSim(await loadSim()), js = new FixedClock(), native = world.clock(new FixedClock());
  for (const seconds of [NaN, Infinity, -1, '0.1', null, undefined]) {
    const fail = () => assert.fail('an invalid duration must not tick');
    assert.deepEqual(invoke(native, seconds, fail, false), invoke(js, seconds, fail, false));
    assert.deepEqual(state(native), state(js));
  }
  for (const clock of [js, native]) {
    assert.throws(() => clock.advance(0.05, (_dt, tick) => {
      if (tick === 2) { clock.maxDebt = 4; throw Error('tick failed'); }
    }, false), /tick failed/);
  }
  assert.deepEqual(state(native), state(js));
  const expected = [], actual = [];
  assert.equal(native.advance(0, (dt, tick) => actual.push([dt, tick]), false), js.advance(0, (dt, tick) => expected.push([dt, tick]), false));
  assert.deepEqual(actual, expected); assert.deepEqual(state(native), state(js));
});
