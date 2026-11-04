import { replayTicks } from '../../src/runtime/reconcile.js';

const viewKeys = ['mouseScale', 'invertMouse', 'firstPerson', 'aspect', 'viewTick', 'flightSettings', 'gamepad'];
export function replayNumberBits(number) { const bytes = Buffer.alloc(8); bytes.writeDoubleLE(number); return bytes.toString('hex'); }
export function replayFixture(native) {
  const ids = new Map(); let serial = 0;
  const f = { native, events: [], pending: [], from: 0, generation: 1, held: [], initialView: {}, retained: [] };
  f.tag = (value, name) => { ids.set(value, name); return value; };
  f.id = value => {
    if ((typeof value !== 'object' || value === null) && typeof value !== 'function' && typeof value !== 'symbol') return value;
    if (!ids.has(value)) ids.set(value, `value-${++serial}`);
    return ids.get(value);
  };
  f.snapshot = (value, seen = new Set()) => {
    if (typeof value === 'number') return { number: value, bits: replayNumberBits(value) };
    if (typeof value === 'symbol' || typeof value === 'function') return { ref: f.id(value), type: typeof value };
    if (value === null || typeof value !== 'object') return value;
    const ref = f.id(value); if (seen.has(value)) return { ref }; seen.add(value);
    const entries = Object.entries(Object.getOwnPropertyDescriptors(value)).filter(([key]) => key !== 'length').map(([key, descriptor]) => [key,
      'value' in descriptor ? f.snapshot(descriptor.value, seen) : { accessor: true }]);
    return { ref, type: Array.isArray(value) ? 'array' : 'object', entries };
  };
  f.frame = (tick, command, sequences) => ({ tick, keys: Object.keys(command), command: f.snapshot(command), sequences: f.snapshot(sequences) });
  f.command = (tick, sequence, extra = {}) => ({ tick, generation: 1, sequence,
    actions: [f.tag({ press: sequence }, `action-${sequence}`)], down: f.tag([`key-${sequence}`], `down-${sequence}`),
    look: [f.tag([sequence, -sequence], `sample-${sequence}`)], ...extra });
  f.tag(f.held, 'initial-held'); f.tag(f.initialView, 'initial-view');
  return f;
}

export function runReplayCase(scenario, native = null) {
  const f = scenario.build(native), frames = [], before = native?.calls ?? 0;
  let returned, thrown = false, exception;
  const apply = function (tick, command, sequences) {
    f.tag(command, `frame-${frames.length}`); f.tag(command.actions, `actions-${frames.length}`); f.tag(command.look, `look-${frames.length}`); f.tag(sequences, `sequences-${frames.length}`);
    frames.push(f.frame(tick, command, sequences)); f.retained.push({ tick, command, sequences });
    f.events.push(['apply', tick, this === undefined]);
    return f.apply?.(tick, command, sequences, frames.length - 1);
  };
  try { returned = replayTicks(f.pending, f.from, f.generation, f.held, Object.hasOwn(f, 'applyValue') ? f.applyValue : apply, f.initialView, { native }); }
  catch (value) {
    thrown = true;
    exception = value instanceof Error ? { name: value.name, message: value.message, same: value === f.throwValue }
      : { value: f.snapshot(value), same: value === f.throwValue };
  } finally { f.cleanup?.(); }
  const result = { returned, thrown, exception, frames, retained: f.retained.map(({ tick, command, sequences }) => f.frame(tick, command, sequences)),
    events: f.events, final: f.final?.() };
  return { result, calls: (native?.calls ?? 0) - before };
}

export function replayOrdinaryCases() {
  const cases = [];
  for (let seed = 0; seed < 40; seed++) cases.push({ name: `ordinary:ordered-${seed}`, native: true, build(native) {
    const f = replayFixture(native); f.from = seed % 4 / 4; f.generation = seed % 3 + 1;
    const gamepad = f.tag({ axes: [0, -0, .25, -.5], buttons: seed }, 'gamepad');
    const flight = f.tag({ sensitivity: .25, curve: 1 }, 'flight');
    f.initialView = { aspect: 1.5, gamepad, mouseScale: -0, flightSettings: flight };
    for (let i = 0; i < 18; i++) {
      const tick = Math.floor(i * 1.25) + (seed % 2 ? .5 : 0);
      const extra = { generation: i % 7 === 0 ? f.generation + 1 : f.generation };
      if (i % 4 === 0) extra.sent = false;
      else if (i % 4 === 1) extra.sent = 0;
      if (i % 5 === 0) extra.worldLook = [f.tag([i / 2, i], `world-${i}`)];
      else if (i % 5 === 1) extra.worldLook = null;
      if (i % 3 === 0) extra.firstPerson = !!(i % 2);
      if (i % 6 === 0) extra.aspect = null;
      if (i % 5 === 0) extra.gamepad = gamepad;
      if (i % 4 === 2) extra.invertMouse = false;
      f.pending.push(f.command(tick, i + 1, extra));
    }
    f.final = () => ({ gamepad: f.snapshot(gamepad), flight: f.snapshot(flight) });
    return f;
  } });
  cases.push({ name: 'ordinary:future-foreign-command-blocks-later-life', native: true, build(native) {
    const f = replayFixture(native); f.pending = [f.command(1, 1), f.command(100, 2, { generation: 2 }), f.command(5, 3)]; return f;
  } });
  cases.push({ name: 'ordinary:unsent-and-unused-fields-are-not-read', native: true, build(native) {
    const f = replayFixture(native), unsent = f.command(1, 1, { sent: false }), foreign = f.command(2, 2, { generation: 2 }), selected = f.command(3, 3, { worldLook: [] });
    for (const key of ['down', 'look', 'worldLook', ...viewKeys]) Object.defineProperty(unsent, key, { get() { throw Error(`unread unsent ${key}`); } });
    for (const key of ['actions', 'sent', 'sequence', 'down', 'look']) Object.defineProperty(foreign, key, { get() { throw Error(`unread foreign ${key}`); } });
    Object.defineProperty(selected, 'look', { get() { throw Error('unused fallback look'); } }); f.pending = [unsent, foreign, selected]; return f;
  } });
  cases.push({ name: 'ordinary:no-life-ignores-view-and-callback', native: true, noFrames: true, build(native) {
    const f = replayFixture(native); f.from = -0; const c = { generation: 2 };
    Object.defineProperty(c, 'tick', { get() { throw Error('unread foreign tick'); } }); f.pending = [c]; f.applyValue = 3;
    for (const key of viewKeys) Object.defineProperty(f.initialView, key, { get() { throw Error('unread initial view'); } }); return f;
  } });
  cases.push({ name: 'ordinary:fractional-endpoint-copies-view-without-frame', native: true, noFrames: true, build(native) {
    const f = replayFixture(native); f.from = 10.5; f.pending = [f.command(10.75, 1)]; f.applyValue = null;
    Object.defineProperty(f.initialView, 'aspect', { get() { f.events.push('view'); return 2; } }); return f;
  } });
  cases.push({ name: 'ordinary:opaque-primitives-keep-negative-zero', native: true, build(native) {
    const f = replayFixture(native); f.held = -0;
    f.pending = [f.command(2, -0, { down: 0, actions: [-0, 0, undefined, null], look: [-0, 0, null], mouseScale: -0, aspect: 0 }), f.command(4, 0, { down: -0, actions: [], look: [] })]; return f;
  } });
  return cases;
}

export function replayMutationCases() {
  const cases = [];
  for (const mode of ['shift', 'unshift', 'replace', 'truncate', 'append-inside', 'append-beyond', 'wrong-life', 'reorder', 'late-accessor']) cases.push({ name: `mutation:${mode}`, native: true, build(native) {
    const f = replayFixture(native); f.pending = [1, 3, 5, 7].map((tick, i) => f.command(tick, i + 1));
    let viewReads = 0;
    Object.defineProperty(f.initialView, 'aspect', { get() { f.events.push(['initial-view', ++viewReads]); return 2; } });
    f.apply = (_tick, _command, _sequences, index) => {
      if (index !== 0) return;
      if (mode === 'shift') f.pending.shift();
      if (mode === 'unshift') f.pending.unshift(f.command(1, 99));
      if (mode === 'replace') f.pending.splice(1, 1, f.command(2, 99));
      if (mode === 'truncate') f.pending.length = 1;
      if (mode === 'append-inside') f.pending.push(f.command(2, 99));
      if (mode === 'append-beyond') f.pending.push(f.command(90, 99));
      if (mode === 'wrong-life') f.pending[1].generation = 2;
      if (mode === 'reorder') [f.pending[1], f.pending[2]] = [f.pending[2], f.pending[1]];
      if (mode === 'late-accessor') Object.defineProperty(f.pending[1], 'tick', { get() { f.events.push('late-tick'); return 3; } });
    };
    f.final = () => ({ viewReads, pendingLength: f.pending.length }); return f;
  } });
  cases.push({ name: 'mutation:held-samples-and-view-object-aliases', native: true, build(native) {
    const f = replayFixture(native), held = f.tag(['held'], 'shared-down'), sample = f.tag([1, 2], 'shared-sample');
    const context = f.tag({ sensitivity: 1, axes: [0, 0], buttons: 1 }, 'shared-context'), action = f.tag({ pressed: true }, 'shared-action');
    f.held = held; f.initialView = { gamepad: context, flightSettings: context };
    f.pending = [f.command(1, 1, { down: held, actions: [action], look: [sample, sample] }), f.command(3, 2, { down: held, actions: [action], worldLook: [sample] })];
    f.apply = (_tick, command, sequences, index) => {
      if (index === 0) { command.down.push('edited'); command.down = f.tag(['replacement'], 'replacement-down'); command.look[0][0] = 99;
        command.gamepad.buttons = 8; command.gamepad = f.tag({ buttons: 16 }, 'replacement-gamepad'); command.flightSettings.sensitivity = 4;
        command.actions[0].pressed = false; command.actions.push('output-only'); sequences.push(100); }
      if (index === 1) f.events.push(['bindings', command.down === held, command.gamepad === context, command.flightSettings === context]);
    };
    f.final = () => ({ held: f.snapshot(held), sample: f.snapshot(sample), context: f.snapshot(context), action: f.snapshot(action),
      freshActions: new Set(f.retained.map(v => v.command.actions)).size, freshLook: new Set(f.retained.map(v => v.command.look)).size }); return f;
  } });
  for (const mode of ['view-setter', 'array-push']) cases.push({ name: `mutation:installed-${mode}`, native: true, build(native) {
    const f = replayFixture(native); f.pending = [f.command(1, 1), f.command(3, 2, { aspect: 2 }), f.command(5, 3)];
    const push = Array.prototype.push, aspect = Object.getOwnPropertyDescriptor(Object.prototype, 'aspect');
    f.cleanup = () => { Array.prototype.push = push; if (aspect) Object.defineProperty(Object.prototype, 'aspect', aspect); else delete Object.prototype.aspect; };
    f.apply = (_tick, _command, _sequences, index) => {
      if (index) return;
      if (mode === 'view-setter') Object.defineProperty(Object.prototype, 'aspect', { configurable: true, set(value) {
        f.events[f.events.length] = ['view-setter', value]; this.aspectSeen = value;
      } });
      else Array.prototype.push = function (...values) {
        f.events[f.events.length] = ['push', typeof values[0], values.length]; return Reflect.apply(push, this, values);
      };
    }; return f;
  } });
  return cases;
}

export function replayOpaqueCases() {
  const patterns = [0x8000000000000000n, 0n, 0x7ff0000000000001n, 0xfff0000000000123n, 0x7ff000000000ff00n, 0x7ff8000000000001n, 0xfff8000000000123n];
  const cases = [];
  for (const pattern of patterns) for (const kind of ['packed', 'tagged', 'typed']) cases.push({ name: `opaque:${kind}-${pattern.toString(16)}`, build(native) {
    const f = replayFixture(native), data = new DataView(new ArrayBuffer(8));
    const number = () => { data.setBigUint64(0, pattern, true); return data.getFloat64(0, true); };
    const elements = () => {
      if (kind === 'typed') { const v = new Float64Array(1); new DataView(v.buffer).setBigUint64(0, pattern, true); return v; }
      if (kind === 'tagged') { const v = [null]; v.push(number()); return v; }
      return [number()];
    };
    f.held = number(); f.initialView = { mouseScale: number(), aspect: number(), gamepad: number(), flightSettings: number() };
    const c = f.command(2, 1); c.sequence = number(); c.down = number(); c.actions = elements(); c.look = elements();
    c.mouseScale = number(); c.invertMouse = number(); c.firstPerson = number(); c.aspect = number(); c.viewTick = number(); c.gamepad = number(); c.flightSettings = number();
    f.pending = [c, f.command(4, 2, { down: number(), actions: [], look: [] })]; return f;
  } });
  return cases;
}

export function replayObservableCases() {
  const cases = [];
  for (const mode of ['different-second-scan', 'iterator-close', 'close-throws']) cases.push({ name: `observable:${mode}`, build(native) {
    const f = replayFixture(native), first = f.command(2, 1), foreign = f.command(3, 2, { generation: 2 }); f.pending = [first, foreign];
    f.throwValue = f.tag({ reason: 'close' }, 'close-error'); let scan = 0;
    f.pending[Symbol.iterator] = function () {
      const pass = ++scan; let at = 0; f.events.push(['iterator', pass]);
      return { next() { f.events.push(['next', pass, at]); const list = mode === 'different-second-scan' && pass === 2 ? [foreign] : [first, foreign]; return at < list.length ? { value: list[at++], done: false } : { done: true }; },
        return() { f.events.push(['return', pass]); if (mode === 'close-throws') throw f.throwValue; return { done: true }; } };
    };
    Object.defineProperty(f.initialView, 'aspect', { get() { f.events.push('view'); return 1; } }); return f;
  } });
  cases.push({ name: 'observable:tick-and-slot-double-reads', build(native) {
    const f = replayFixture(native), first = f.command(1, 1), replacement = f.command(1, 2); let reads = 0, slots = 0;
    Object.defineProperty(first, 'tick', { get() { f.events.push(['tick', ++reads]); if (reads === 3) Object.defineProperty(f.pending, 0, { value: replacement, writable: true, configurable: true }); return reads === 1 ? 5 : reads === 2 ? 2 : 1; } });
    f.pending = [first]; Object.defineProperty(f.pending, 0, { configurable: true, get() { f.events.push(['slot', ++slots]); return first; } }); return f;
  } });
  cases.push({ name: 'observable:double-view-reads-presence-and-insertion-order', build(native) {
    const f = replayFixture(native), c = f.command(2, 1); let initial = 0, command = 0;
    Object.defineProperty(f.initialView, 'aspect', { get() { f.events.push(['initial-aspect', ++initial]); return initial === 1 ? 2 : undefined; } });
    f.initialView.gamepad = f.tag({ buttons: 1 }, 'view-pad');
    Object.defineProperty(c, 'mouseScale', { get() { f.events.push(['command-mouse', ++command]); return command === 1 ? 3 : undefined; } });
    c.invertMouse = false; c.aspect = null; f.pending = [c]; return f;
  } });
  for (const sent of [false, 0, null, undefined]) cases.push({ name: `observable:action-iterator-changes-sent-${String(sent)}`, build(native) {
    const f = replayFixture(native), c = f.command(1, 1), action = f.tag({ trigger: 1 }, 'iterated-action');
    c.actions = { [Symbol.iterator]() { f.events.push('actions-iterator'); let at = 0; return { next() { f.events.push(['actions-next', at]); if (at++ === 0) { c.sent = sent; c.down = f.tag(['changed'], 'changed-down'); return { value: action, done: false }; } return { done: true }; } }; } };
    Object.defineProperty(c, 'worldLook', { get() { f.events.push('world-look'); return null; } });
    Object.defineProperty(c, 'look', { get() { f.events.push('look'); return []; } }); f.pending = [c]; return f;
  } });
  cases.push({ name: 'observable:future-foreign-tick-getter-is-read-only-in-consumption', build(native) {
    const f = replayFixture(native), foreign = { generation: 2 }, mine = f.command(3, 1);
    Object.defineProperty(foreign, 'tick', { get() { f.events.push('foreign-tick'); return 100; } }); f.pending = [foreign, mine]; return f;
  } });
  cases.push({ name: 'observable:initial-view-mutates-pending-after-last-is-captured', native: true, build(native) {
    const f = replayFixture(native); f.pending = [f.command(2, 1)]; let at = 0;
    Object.defineProperty(f.initialView, 'aspect', { get() { f.events.push(['view', ++at]); if (at === 1) f.pending.push(f.command(8, 2)); return 2; } }); return f;
  } });
  cases.push({ name: 'observable:callback-callability-is-checked-after-frame-reads', build(native) {
    const f = replayFixture(native), c = f.command(1, 1); f.applyValue = 3;
    Object.defineProperty(c, 'sent', { get() { f.events.push('sent-before-bad-apply'); return false; } }); f.pending = [c]; return f;
  } });
  for (const target of ['array', 'object']) cases.push({ name: `observable:indexed-prototype-${target}`, build(native) {
    const f = replayFixture(native); f.pending = [f.command(1, 1), f.command(2, 2)];
    const prototype = target === 'array' ? Array.prototype : Object.prototype, previous = Object.getOwnPropertyDescriptor(prototype, '0');
    f.cleanup = () => { if (previous) Object.defineProperty(prototype, '0', previous); else delete prototype[0]; };
    Object.defineProperty(prototype, '0', { configurable: true, set(value) {
      Object.defineProperty(f.events, f.events.length, { configurable: true, writable: true, enumerable: true, value: ['indexed-setter', Array.isArray(this), typeof value] });
      Object.defineProperty(this, '0', { configurable: true, writable: true, enumerable: true, value });
    } });
    return f;
  } });
  cases.push({ name: 'observable:action-body-throw-closes-iterator-with-original-value', native: true, build(native) {
    const f = replayFixture(native), c = f.command(1, 1), push = Array.prototype.push, action = f.tag({ fire: true }, 'throwing-action');
    f.throwValue = f.tag(Symbol('action-stop'), 'action-stop'); f.cleanup = () => { Array.prototype.push = push; };
    const event = value => { f.events[f.events.length] = value; };
    c.actions = { [Symbol.iterator]() { event('actions-iterator'); return {
      next() { event('actions-next'); Array.prototype.push = function (value) { if (value === action) { event('push-throw'); throw f.throwValue; } return Reflect.apply(push, this, [value]); }; return { value: action, done: false }; },
      return() { event('actions-return'); throw Error('secondary close error'); },
    }; } };
    f.pending = [c]; return f;
  } });
  return cases;
}

export function replayBoundaryCases() {
  const cases = [];
  for (const from of [0, -0, .25, -1.25, NaN, Infinity, -Infinity, 2 ** 53]) cases.push({ name: `boundary:from-${String(from)}${Object.is(from, -0) ? '-negative-zero' : ''}`, build(native) {
    const f = replayFixture(native); f.from = from; f.pending = [f.command(2, 1)]; f.throwValue = f.tag(Symbol('bounded-stop'), 'bounded-stop');
    f.apply = (_tick, _command, _sequences, index) => { if (index === 2) throw f.throwValue; }; return f;
  } });
  for (const value of [undefined, null, false, 0, -0, 'stop', Symbol('stop'), { reason: 'stop' }]) cases.push({ name: `boundary:throw-${typeof value}-${String(value)}`, native: true, build(native) {
    const f = replayFixture(native); f.pending = [f.command(2, 1)]; f.throwValue = value;
    if (value !== null && (typeof value === 'object' || typeof value === 'symbol')) f.tag(value, 'thrown-value');
    f.apply = () => { throw value; }; return f;
  } });
  cases.push({ name: 'boundary:coercion-order-is-original', build(native) {
    const f = replayFixture(native); f.from = { [Symbol.toPrimitive](hint) { f.events.push(['from', hint]); return 0; } };
    f.pending = [f.command(2, 1)]; return f;
  } });
  cases.push({ name: 'boundary:no-matching-nan-generation', build(native) {
    const f = replayFixture(native); f.generation = NaN; f.pending = [f.command(1, 1, { generation: NaN })]; f.applyValue = null; return f;
  } });
  return cases;
}

export function replayNestedCases() {
  return [{ name: 'nested:interleaved-owners-and-shared-pending', native: true, build(native) {
    const f = replayFixture(native); f.pending = [f.command(1, 1), f.command(3, 2)]; let active = false;
    f.apply = (tick, _command, _sequences, index) => {
      if (index || active) return; active = true;
      try {
        const returned = replayTicks(f.pending, tick, 1, f.held, (innerTick, inner, sequences) => {
          f.events.push(['nested', innerTick, f.snapshot(inner.down), f.snapshot(inner.actions), f.snapshot(sequences)]);
          if (innerTick === 2) f.pending[1].down.push('nested-edit');
        }, f.initialView, { native }); f.events.push(['nested-return', returned]);
      } finally { active = false; }
    }; return f;
  } }];
}

export const replayCases = () => [...replayOrdinaryCases(), ...replayMutationCases(), ...replayObservableCases(), ...replayBoundaryCases(), ...replayNestedCases(), ...replayOpaqueCases()];
