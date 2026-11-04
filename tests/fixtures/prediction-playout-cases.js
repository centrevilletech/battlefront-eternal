import { PlayoutClock } from '../../src/runtime/playout.js';
import { SnapshotInterval, renderedViewTick } from '../../src/runtime/snapshot-interval.js';

const F64 = Float64Array, raw = new DataView(new ArrayBuffer(8));
const typed = Object.getOwnPropertyDescriptors(Object.getPrototypeOf(F64.prototype));
const fields = ['hz', 'minDelay', 'maxDelay', 'margin', 'window', 'trim', 'snap', 'clock', 'latest', 'delay', 'rate', 'gapAt', 'gapCount', 'arrivedAt', 'arrivedTick', 'started', 'waits', 'measuredKey'];
export const playoutBits = number => { raw.setFloat64(0, number, true); return raw.getBigUint64(0, true).toString(16).padStart(16, '0'); };
const fromBits = bits => { raw.setBigUint64(0, bits, true); return raw.getFloat64(0, true); };
const scalar = value => typeof value === 'number' ? { number: value, bits: playoutBits(value) } : value === null || typeof value !== 'object' && typeof value !== 'function' && typeof value !== 'symbol' ? value : { type: typeof value };
const own = (value, key) => { const d = Object.getOwnPropertyDescriptor(value, key); return d ? Object.hasOwn(d, 'value') ? scalar(d.value) : { accessor: true } : { missing: true }; };
export function playoutRing(ring) {
  try {
    const buffer = Reflect.apply(typed.buffer.get, ring, []), offset = Reflect.apply(typed.byteOffset.get, ring, []), length = Reflect.apply(typed.byteLength.get, ring, []);
    const bytes = new Uint8Array(buffer, offset, length); let hex = '';
    for (let i = 0; i < bytes.length; i++) hex += bytes[i].toString(16).padStart(2, '0');
    return hex;
  } catch { return 'not-readable-typed-storage'; }
}
export function playoutState(value) {
  const result = Object.fromEntries(fields.map(key => [key, own(value, key)])), d = Object.getOwnPropertyDescriptor(value, 'gaps');
  result.gaps = d && Object.hasOwn(d, 'value') ? playoutRing(d.value) : { accessor: true }; return result;
}
export const intervalState = value => ({ previous: own(value, 'previous'), current: own(value, 'current') });
export function playoutAttempt(fn, token) {
  try { return { value: scalar(fn()), thrown: false }; }
  catch (error) { return { thrown: true, same: error === token, error: error instanceof Error ? { name: error.name, message: error.message } : scalar(error) }; }
}
const seedRing = ring => {
  const view = new DataView(ring.buffer, ring.byteOffset, ring.byteLength), patterns = [0x7ff0000000000001n, 0xfff0000000000123n, 0x8000000000000000n, 1n];
  for (let i = 0; i < ring.length; i++) view.setBigUint64(i * 8, patterns[i % patterns.length], true);
};

export function playoutNumericCases() {
  const cases = [];
  for (const [name, config] of [['default', {}], ['short', { window: 3 }], ['one', { window: 1 }], ['empty', { window: 0 }], ['zero-hz', { hz: 0 }], ['reversed-delay', { minDelay: 10, maxDelay: 2 }], ['negative-trim', { trim: -.1 }], ['zero-snap', { snap: 0 }], ['zero-delay', { minDelay: 0, maxDelay: 0, margin: -0 }]]) {
    cases.push({ name: `clock:${name}`, native: true, run(native) {
      const clock = new PlayoutClock(config, { native }), bracket = new SnapshotInterval({ native }), observations = [];
      const old = clock.gaps; seedRing(old); const oldBytes = playoutRing(old); clock.reset(); const ring = clock.gaps;
      observations.push({ reset: playoutState(clock), replaced: ring !== old, oldBytes, oldAfter: playoutRing(old), keys: Object.keys(clock) });
      observations.push({ value: scalar(clock.advance(0)), shown: scalar(clock.shownTick), state: playoutState(clock) });
      for (let i = 0; i < 180; i++) {
        const tick = i % 19 === 0 ? i - 3 : i * 3, now = i / 20 + (i % 17 > 12 ? .15 : .005), interval = [1, 3, 0, -1][i % 4];
        clock.arrive(tick, now, interval);
        const shown = clock.advance([1 / 60, 1 / 144, 1 / 20, 0, -.01, .5][i % 6]);
        const needs = bracket.needsSample(shown); if (needs && i % 4 !== 0) bracket.apply(tick);
        observations.push({ shown: scalar(shown), shownTick: scalar(clock.shownTick), needs, alpha: scalar(bracket.alpha(shown)), view: scalar(renderedViewTick(shown, tick, { native })),
          clock: playoutState(clock), bracket: intervalState(bracket), ring: clock.gaps === ring });
      }
      return observations;
    } });
  }
  for (const window of [2.5, NaN, -1, Infinity, '3', null]) cases.push({ name: `clock:window-${String(window)}`, run(native) {
    let clock; class Captured extends PlayoutClock { reset() { clock = this; super.reset(); } }
    const result = playoutAttempt(() => new Captured({ window }, { native })), observations = [{ result, state: playoutState(clock) }];
    if (!result.thrown) for (let i = 0; i < 8; i++) { clock.arrive(i, i / 60); observations.push({ value: scalar(clock.advance(1 / 60)), state: playoutState(clock) }); }
    return observations;
  } });
  for (const current of [null, undefined, -0, 0, .25, 1, -1, Number.MAX_SAFE_INTEGER, Infinity, -Infinity, NaN]) cases.push({ name: `interval:current-${String(current)}${Object.is(current, -0) ? '-negative-zero' : ''}`, run(native) {
    const interval = new SnapshotInterval({ native }), out = [];
    for (const previous of [null, undefined, -0, 0, 1, 2.5, Infinity, NaN]) for (const clock of [-1, -0, .25, 2, Infinity, NaN]) {
      interval.previous = previous; interval.current = current;
      out.push({ needs: interval.needsSample(clock), alpha: scalar(interval.alpha(clock)), state: intervalState(interval) });
    }
    interval.apply(current); out.push(intervalState(interval)); interval.reset(); out.push(intervalState(interval)); return out;
  } });
  cases.push({ name: 'rendered:finite-guard-and-floor-boundaries', native: true, run(native) {
    const values = [-Infinity, -2.5, -0, 0, .999, 1, 12.25, Number.MAX_VALUE, Infinity, NaN, null, undefined, '3', { valueOf() { throw Error('must not coerce'); } }];
    const out = []; for (const clock of values) for (const latest of values) out.push(scalar(renderedViewTick(clock, latest, { native }))); return out;
  } });
  return cases;
}

export function playoutCallbackCases() {
  const cases = [];
  for (const mode of ['new', 'old', 'throw', 'lookup']) cases.push({ name: `reset:constructor-${mode}`, native: true, run(native) {
    const clock = new PlayoutClock({ window: 3 }, { native }), old = clock.gaps, events = [], token = Symbol('constructor-stop');
    Object.assign(clock, { clock: 50, latest: 60, delay: 9, rate: 2, gapAt: 2, gapCount: 3, started: true, waits: 4, measuredKey: 8 });
    const original = Object.getOwnPropertyDescriptor(globalThis, 'Float64Array'); let allocated;
    function Replacement(length) {
      events.push(['construct', length, playoutState(clock), clock.gaps === old]); clock.clock = 88; clock.gapAt = 99;
      if (mode === 'throw') throw token;
      return allocated = mode === 'old' ? old : new F64(length);
    }
    if (mode === 'lookup') {
      Object.defineProperty(clock, 'window', { configurable: true, get() { events.push('window'); return 4; } });
      Object.defineProperty(globalThis, 'Float64Array', { configurable: true, get() { events.push('constructor-lookup'); return Replacement; } });
    } else Object.defineProperty(globalThis, 'Float64Array', { configurable: true, writable: true, value: Replacement });
    let result;
    try { result = playoutAttempt(() => clock.reset(), token); }
    finally { Object.defineProperty(globalThis, 'Float64Array', original); }
    return { events, result, state: playoutState(clock), old: clock.gaps === old, allocated: clock.gaps === allocated };
  } });
  cases.push({ name: 'reset:prefix-setter-throw-precedes-ring-allocation', run(native) {
    const clock = new PlayoutClock({}, { native }), old = clock.gaps, events = [], token = { reset: 'stop' }; let latest = 10;
    clock.clock = 20;
    Object.defineProperty(clock, 'latest', { configurable: true, get: () => latest, set(value) { events.push(['latest', value, clock.clock]); latest = value; throw token; } });
    const result = playoutAttempt(() => clock.reset(), token); return { result, events, latest, old: clock.gaps === old, state: playoutState(clock) };
  } });
  cases.push({ name: 'arrival:latest-precedes-live-arrival-and-interval-coercion', run(native) {
    const clock = new PlayoutClock({ window: 3 }, { native }), events = []; clock.arrive(1, .1); let reads = 0;
    Object.defineProperty(clock, 'arrivedAt', { configurable: true, get() { events.push(['arrived', ++reads, clock.latest]); if (reads === 2) clock.hz = 30; return .1; }, set(value) { events.push(['arrived-set', value]); } });
    const interval = { valueOf() { events.push(['interval', clock.hz]); clock.gapAt = 2; return 3; } };
    clock.arrive(5, .5, interval); return { events, state: playoutState(clock) };
  } });
  cases.push({ name: 'measure:key-published-before-slice-sort-and-live-index', native: true, run(native) {
    const clock = new PlayoutClock({ window: 5 }, { native }), events = []; clock.gaps.set([5, 2, 1, 9, 4]); clock.gapCount = 5; clock.gapAt = 3;
    clock.gaps.slice = function (start, end) {
      events.push(['slice', start, end, clock.measuredKey]);
      return { sort() { events.push('sort'); clock.gapCount = 2; clock.margin = 3; return [4, 8, 12]; } };
    };
    const result = clock.measure(); return { result: scalar(result), events, state: playoutState(clock) };
  } });
  cases.push({ name: 'measure:species-sees-key-and-changes-later-policy', native: true, run(native) {
    const clock = new PlayoutClock({ window: 4 }, { native }), events = []; clock.gaps.set([8, 3, 6, 1]); clock.gapCount = 4;
    Object.defineProperty(clock.gaps, 'constructor', { value: { get [Symbol.species]() { events.push(['species', clock.measuredKey]); return function (length) { events.push(['allocate', length]); clock.gapCount = 1; clock.margin = 4; return new F64(length); }; } } });
    const result = clock.measure(); return { result: scalar(result), events, state: playoutState(clock) };
  } });
  cases.push({ name: 'measure:throw-retains-key-and-cached-call-skips-all-work', native: true, run(native) {
    const clock = new PlayoutClock({ window: 3 }, { native }), events = [], token = null; clock.gapCount = 2; clock.gapAt = 1; clock.delay = 7;
    clock.gaps.slice = () => { events.push(['slice', clock.measuredKey]); throw token; };
    const first = playoutAttempt(() => clock.measure(), token);
    for (const key of ['gaps', 'minDelay', 'maxDelay', 'margin']) Object.defineProperty(clock, key, { configurable: true, get() { events.push(['unread', key]); throw Error(`unread ${key}`); } });
    const second = playoutAttempt(() => clock.measure()); return { first, second, events, state: playoutState(clock) };
  } });
  cases.push({ name: 'measure:delay-clamp-read-and-coercion-order', run(native) {
    const clock = new PlayoutClock({ window: 1 }, { native }), events = []; clock.gapCount = 1;
    const gap = { valueOf() { events.push('gap-value'); return 4; } };
    clock.gaps.slice = () => ({ sort: () => [gap] });
    for (const [key, value] of [['maxDelay', 9], ['minDelay', 2], ['margin', 3]]) Object.defineProperty(clock, key, { configurable: true, get() { events.push(key); return value; } });
    const result = clock.measure(); return { result: scalar(result), events, state: playoutState(clock) };
  } });
  cases.push({ name: 'advance:measure-override-reentry-and-late-field-reads', native: true, run(native) {
    const clock = new PlayoutClock({}, { native }), events = []; let entered = false;
    clock.measure = function () {
      events.push(['measure', this.clock, this.started]);
      if (!entered) { entered = true; this.latest = 10; this.delay = 3; const nested = this.advance(0); events.push(['nested', nested]); this.latest = 11; this.delay = 2; this.snap = 1; }
      return -999;
    };
    const result = clock.advance(.25); return { result: scalar(result), events, state: playoutState(clock) };
  } });
  cases.push({ name: 'advance:startup-and-return-read-setter-order', run(native) {
    const clock = new PlayoutClock({}, { native }), events = []; let current = 4, started = false, rate = .5;
    clock.measure = () => { events.push('measure'); }; clock.latest = 10; clock.delay = 3;
    Object.defineProperties(clock, {
      started: { configurable: true, get() { events.push('started-get'); return started; }, set(value) { events.push(['started-set', value]); started = value; clock.latest = 100; } },
      clock: { configurable: true, get() { events.push(['clock-get', current]); return current; }, set(value) { events.push(['clock-set', value]); current = value + .25; } },
      rate: { configurable: true, get: () => rate, set(value) { events.push(['rate-set', value]); rate = value; current += .5; } },
    });
    const result = clock.advance(.1); return { result: scalar(result), events, current: scalar(current), started, rate: scalar(rate) };
  } });
  cases.push({ name: 'advance:measure-throw-has-no-later-publication', run(native) {
    const clock = new PlayoutClock({}, { native }), token = Symbol('measure-stop'); clock.latest = 10;
    clock.measure = () => { clock.delay = 4; throw token; };
    const result = playoutAttempt(() => clock.advance(.1), token); return { result, state: playoutState(clock) };
  } });
  return cases;
}

export function intervalCallbackCases() {
  return [
    { name: 'interval:nullable-checks-skip-later-access-and-coercion', run(native) {
      const interval = new SnapshotInterval({ native }), events = [];
      Object.defineProperty(interval, 'current', { configurable: true, get() { events.push('current'); return null; } });
      Object.defineProperty(interval, 'previous', { configurable: true, get() { throw Error('unread previous'); } });
      const clock = { valueOf() { throw Error('unread clock'); } };
      return { needs: interval.needsSample(clock), alpha: scalar(interval.alpha(clock)), events };
    } },
    { name: 'interval:double-current-and-previous-reads', run(native) {
      const interval = new SnapshotInterval({ native }), events = []; let current = 0, previous = 0;
      Object.defineProperties(interval, {
        current: { configurable: true, get() { events.push(['current', ++current]); return current === 1 ? 10 : 3; } },
        previous: { configurable: true, get() { events.push(['previous', ++previous]); return previous === 1 ? 1 : 2; } },
      });
      const clock = { valueOf() { events.push('clock-value'); return 2; } }, alpha = interval.alpha(clock);
      return { alpha: scalar(alpha), events };
    } },
    { name: 'interval:apply-selective-publication-and-throw', run(native) {
      const out = [];
      for (const phase of ['previous', 'current', 'none']) {
        const interval = new SnapshotInterval({ native }), token = Symbol('apply-stop'), events = []; let previous = 0, current = null;
        Object.defineProperties(interval, {
          previous: { configurable: true, get: () => previous, set(value) { events.push(['previous-set', scalar(value)]); previous = value; current = 99; if (phase === 'previous') throw token; } },
          current: { configurable: true, get() { events.push(['current-get', current]); return current; }, set(value) { events.push(['current-set', scalar(value)]); current = value; if (phase === 'current') throw token; } },
        });
        const tick = { valueOf() { events.push('tick-value'); return 5; } }, result = playoutAttempt(() => interval.apply(tick), token);
        out.push({ phase, events, result, previous: scalar(previous), current: scalar(current), same: current === tick });
      }
      return out;
    } },
    { name: 'interval:reset-calls-inherited-setters-in-order', run(native) {
      const events = []; let previous = 8, current = 9;
      class Interval extends SnapshotInterval {
        get previous() { return previous; } set previous(value) { events.push(['previous', value, current]); previous = value; current = 20; }
        get current() { return current; } set current(value) { events.push(['current', value, previous]); current = value; }
      }
      const interval = new Interval({ native }); previous = 4; current = 5; interval.reset(); return { events, previous, current, keys: Object.keys(interval) };
    } },
  ];
}

export const playoutCases = () => [...playoutNumericCases(), ...playoutCallbackCases(), ...intervalCallbackCases()];
export function runPlayoutCase(scenario, native = null) {
  const calls = native?.calls ?? 0, result = scenario.run(native);
  return { result, calls: (native?.calls ?? 0) - calls };
}
