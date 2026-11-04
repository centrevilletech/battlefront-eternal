import { bridgeFor } from '../sim/runtime.js';

const unsupported = Symbol('observable playout state');
const own = Object.hasOwn, getDescriptor = Object.getOwnPropertyDescriptor;
const getPrototype = Object.getPrototypeOf, extensible = Object.isExtensible;
const defineProperty = Object.defineProperty, apply = Reflect.apply;
const F64 = Float64Array, U8 = Uint8Array, View = DataView, Decoder = TextDecoder;
const floatPrototype = F64.prototype, typedPrototype = getPrototype(floatPrototype);
const floatSlice = floatPrototype.slice, floatSort = floatPrototype.sort;
const floatSpecies = getDescriptor(getPrototype(F64), Symbol.species);
const typedTag = getDescriptor(typedPrototype, Symbol.toStringTag);
const typedLength = getDescriptor(typedPrototype, 'length');
const typedBuffer = getDescriptor(typedPrototype, 'buffer');
const typedOffset = getDescriptor(typedPrototype, 'byteOffset');
const typedBytes = getDescriptor(typedPrototype, 'byteLength');
const arrayBufferBytes = getDescriptor(ArrayBuffer.prototype, 'byteLength').get;
const readFloat = View.prototype.getFloat64, decodeText = Decoder.prototype.decode;
const MathObject = Math, NumberObject = Number;
const min = Math.min, max = Math.max, floor = Math.floor, abs = Math.abs;
const finite = Number.isFinite, integer = Number.isSafeInteger;
const bound = Number.MAX_SAFE_INTEGER;

function append(array, value) {
  defineProperty(array, array.length, { __proto__: null, value, writable: true, enumerable: true, configurable: true });
}
function descriptor(value, key) {
  for (let at = value; at; at = getPrototype(at)) {
    const found = getDescriptor(at, key);
    if (found) return found;
  }
}
function object(value) { return value !== null && (typeof value === 'object' || typeof value === 'function'); }
function data(value, key) {
  if (!object(value)) throw unsupported;
  const found = descriptor(value, key);
  if (!found) return undefined;
  if (!own(found, 'value')) throw unsupported;
  return found.value;
}
function writable(value, keys, owned = true) {
  if (!object(value)) throw unsupported;
  for (let i = 0; i < keys.length; i++) {
    const local = getDescriptor(value, keys[i]), found = local || descriptor(value, keys[i]);
    if (owned && !local || found && (!own(found, 'value') || !found.writable)
      || !local && !extensible(value)) throw unsupported;
  }
}
// These are eligibility bounds, never clamps. Exceptional numeric state keeps
// the original phase, including JavaScript's NaN payload and coercion rules.
function bounded(value) { return typeof value === 'number' && value >= -bound && value <= bound; }
function number(value, key) {
  const n = data(value, key);
  if (!bounded(n)) throw unsupported;
  return n;
}
function nullable(value) {
  if (value !== null && !bounded(value)) throw unsupported;
  return value === null ? [0, 0] : [1, value];
}
function math(keys) {
  if (data(globalThis, 'Math') !== MathObject) throw unsupported;
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i], expected = key === 'min' ? min : key === 'max' ? max : key === 'floor' ? floor : abs;
    if (data(MathObject, key) !== expected) throw unsupported;
  }
}
function accessor(found, expected) {
  return found && !own(found, 'value') && own(found, 'get') && own(found, 'set')
    && found.get === expected.get && found.set === expected.set;
}
function ring(value, memory, sorted = false) {
  if (!object(value) || apply(typedTag.get, value, []) !== 'Float64Array'
    || getPrototype(value) !== floatPrototype
    || !accessor(descriptor(value, 'length'), typedLength)
    || !accessor(descriptor(value, 'buffer'), typedBuffer)
    || !accessor(descriptor(value, 'byteOffset'), typedOffset)
    || !accessor(descriptor(value, 'byteLength'), typedBytes)) throw unsupported;
  const buffer = apply(typedBuffer.get, value, []);
  if (buffer === memory.buffer) throw unsupported;
  // The ordinary ArrayBuffer getter rejects shared storage without invoking
  // a caller's constructor, tag or custom byteLength accessor.
  try { apply(arrayBufferBytes, buffer, []); } catch { throw unsupported; }
  if (sorted && (data(value, 'slice') !== floatSlice || data(floatPrototype, 'sort') !== floatSort
    || data(value, 'constructor') !== F64 || !accessor(descriptor(F64, Symbol.species), floatSpecies))) throw unsupported;
  return { value, length: apply(typedLength.get, value, []) };
}
function metadata(make) {
  try { return make(); }
  catch (error) { if (error === unsupported) return null; throw error; }
}
function reader(bytes) {
  const view = new View(apply(typedBuffer.get, bytes, []), apply(typedOffset.get, bytes, []), apply(typedBytes.get, bytes, [])); let at = 0;
  return () => { const value = apply(readFloat, view, [at, true]); at += 8; return value; };
}
function copyWords(words) {
  const result = [];
  for (let i = 0; i < words.length; i++) append(result, words[i]);
  return result;
}

export function createPlayoutSim(instance, options) { return createPlayoutBridge(bridgeFor(instance), options); }

// One reusable native calculation owner. Models and gap arrays stay with the
// caller; each phase owns its output before publication or a trace callback.
export function createPlayoutBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, decoder = new Decoder(), continuation = {}; let handle = 0, calls = 0, disposed = false;
  const live = () => { if (disposed) throw Error('playout bridge is disposed'); };
  function prepare(make, token) {
    if (disposed && token === continuation) return null;
    live(); const result = metadata(make);
    if (disposed && token === continuation) return null;
    live(); return result;
  }
  function copyBytes(at, length) {
    const source = new U8(x.memory.buffer, at, length), result = new U8(length);
    for (let i = 0; i < length; i++) result[i] = source[i];
    return result;
  }
  function run(op, words, unpack) {
    live();
    if (!handle) handle = x.prediction_playout_create();
    const at = x.prediction_playout_room(handle, words.length);
    // Use captured constructors for transport. reset() alone performs its
    // original live Float64Array lookup and allocation between native phases.
    const input = new F64(x.memory.buffer, at, words.length);
    for (let i = 0; i < words.length; i++) input[i] = words[i];
    const ok = x.prediction_playout_run(handle, op); calls++;
    const outputBytes = x.prediction_playout_output_len(handle) * 8;
    const output = copyBytes(x.prediction_playout_output(handle), outputBytes);
    const error = ok ? null : apply(decodeText, decoder, [copyBytes(x.prediction_playout_error(handle), x.prediction_playout_error_len(handle))]);
    const result = ok ? unpack(reader(output)) : null;
    if (trace) {
      const values = [], read = reader(output);
      for (let i = 0; i < outputBytes; i += 8) append(values, read());
      trace({ op, input: copyWords(words), output: values, error });
    }
    if (!ok) throw Error(error);
    return result;
  }
  return {
    bridge,
    get calls() { return calls; },
    get handle() { return handle; },
    // A token permits only the remaining JavaScript phases after close. A new
    // invocation still fails at entry, and a closed owner never re-enters Rust.
    entry() { live(); return continuation; },
    resetHead(value, token) {
      const words = prepare(() => {
        writable(value, ['clock', 'latest', 'delay', 'rate'], false);
        return [number(value, 'minDelay')];
      }, token);
      return words ? run(0, words, read => {
        value.clock = read(); value.latest = read(); value.delay = read(); value.rate = read(); return true;
      }) : false;
    },
    resetTail(value, token) {
      if (!prepare(() => {
        writable(value, ['gapAt', 'gapCount', 'arrivedAt', 'arrivedTick', 'started', 'waits', 'measuredKey'], false);
        return true;
      }, token)) return false;
      return run(1, [], read => {
        value.gapAt = read(); value.gapCount = read(); const present = read(), at = read(); value.arrivedAt = present ? at : null;
        value.arrivedTick = read(); value.started = !!read(); value.waits = read(); value.measuredKey = read(); return true;
      });
    },
    arrive(value, tick, now, interval) {
      const state = prepare(() => {
        if (!bounded(tick) || !bounded(now) || !bounded(interval)) throw unsupported;
        math(['max']);
        const hz = number(value, 'hz'), window = number(value, 'window'), latest = number(value, 'latest');
        const arrived = nullable(data(value, 'arrivedAt')), arrivedTick = number(value, 'arrivedTick');
        const at = number(value, 'gapAt'), count = number(value, 'gapCount'), gaps = ring(data(value, 'gaps'), x.memory);
        if (!(hz > 0) || !integer(window) || window <= 0 || !integer(at) || at < 0 || at >= window
          || !integer(count) || count < 0 || count > window) throw unsupported;
        writable(value, ['latest', 'gapAt', 'gapCount', 'arrivedAt', 'arrivedTick']);
        return { gaps, words: [hz, window, latest, arrived[0], arrived[1], arrivedTick, at, count, gaps.length, tick, now, interval] };
      });
      if (!state) return false;
      return run(2, state.words, read => {
        const latest = read(), latestValue = read(), advance = read(), index = read(), entry = read();
        const at = read(), count = read(), countValue = read(); read(); const arrivedTick = read();
        if (latest) value.latest = latestValue;
        if (advance) {
          if (index >= 0) state.gaps.value[index] = entry;
          value.gapAt = at;
          if (count) value.gapCount = countValue;
        }
        value.arrivedAt = now; value.arrivedTick = arrivedTick; return true;
      });
    },
    measureKey(value, token) {
      const words = prepare(() => {
        writable(value, ['measuredKey']);
        return [number(value, 'gapCount'), number(value, 'gapAt'), number(value, 'measuredKey')];
      }, token);
      return words ? run(3, words, read => {
        const changed = !!read(), key = read();
        if (changed) value.measuredKey = key;
        return { changed, key };
      }) : null;
    },
    measureGap(value, token) {
      const words = prepare(() => {
        math(['min', 'floor']);
        const count = number(value, 'gapCount'), gaps = ring(data(value, 'gaps'), x.memory, true);
        if (!integer(count) || count < 0 || count > gaps.length) throw unsupported;
        const words = [count];
        for (let i = 0; i < count; i++) {
          const sample = gaps.value[i];
          if (!bounded(sample)) throw unsupported;
          append(words, sample);
        }
        return words;
      }, token);
      return words ? run(4, words, read => ({ value: read() })) : null;
    },
    measureDelay(value, gap, token) {
      const words = prepare(() => {
        if (!bounded(gap)) throw unsupported;
        math(['min', 'max']); writable(value, ['delay']);
        const maximum = number(value, 'maxDelay'), minimum = number(value, 'minDelay'), margin = number(value, 'margin');
        return [gap, margin, minimum, maximum];
      }, token);
      return words ? run(5, words, read => { value.delay = read(); return true; }) : false;
    },
    advance(value, dt, token) {
      const words = prepare(() => {
        if (!bounded(dt)) throw unsupported;
        math(['min', 'max', 'abs']);
        const latest = number(value, 'latest'), delay = number(value, 'delay'), clock = number(value, 'clock');
        const started = data(value, 'started'), hz = number(value, 'hz'), trim = number(value, 'trim');
        const snap = number(value, 'snap'), waits = number(value, 'waits');
        if (typeof started !== 'boolean' || hz < 1 / bound) throw unsupported;
        writable(value, ['started', 'clock', 'rate', 'waits']);
        return [latest, delay, clock, +started, hz, trim, snap, dt, waits];
      }, token);
      return words ? run(6, words, read => {
        const mode = read(), clock = read(), rate = read(), waits = read();
        if (mode === 1) { value.started = true; value.clock = clock; value.rate = rate; }
        else if (mode === 2) { value.clock = clock; value.rate = rate; }
        else if (mode === 3 || mode === 4) {
          value.rate = rate;
          if (mode === 4) value.waits = waits;
          value.clock = clock;
        }
        return true;
      }) : false;
    },
    shownTick(value) {
      const words = prepare(() => { math(['floor']); return [number(value, 'clock')]; });
      return words ? run(7, words, read => ({ value: read() })) : null;
    },
    intervalReset(value) {
      if (!prepare(() => { writable(value, ['previous', 'current'], false); return true; })) return false;
      return run(8, [], read => {
        const previous = read(), p = read(), current = read(), c = read();
        value.previous = previous ? p : null; value.current = current ? c : null; return true;
      });
    },
    intervalApply(value, tick) {
      const words = prepare(() => {
        if (!bounded(tick)) throw unsupported;
        writable(value, ['previous', 'current']);
        const current = data(value, 'current');
        if (current !== null && current !== undefined && !bounded(current)) throw unsupported;
        return [+(current != null), current ?? 0, tick];
      });
      return words ? run(9, words, read => { value.previous = read(); value.current = read(); return true; }) : false;
    },
    needsSample(value, clock) {
      const words = prepare(() => {
        const current = data(value, 'current');
        if (current === null) return [0, 0, 0];
        if (!bounded(current) || !bounded(clock)) throw unsupported;
        return [1, current, clock];
      });
      return words ? run(10, words, read => ({ value: !!read() })) : null;
    },
    alpha(value, clock) {
      const words = prepare(() => {
        const current = data(value, 'current');
        if (current === null) return [0, 0, 0, 0, 0];
        if (!bounded(current) || !bounded(clock)) throw unsupported;
        math(['max', 'min']);
        const previous = nullable(data(value, 'previous'));
        return [1, current, previous[0], previous[1], clock];
      });
      return words ? run(11, words, read => ({ value: read() })) : null;
    },
    renderedViewTick(clock, latest) {
      const words = prepare(() => {
        if (data(globalThis, 'Number') !== NumberObject || data(NumberObject, 'isFinite') !== finite) throw unsupported;
        if (!finite(clock)) return [0, 0, 0, 0];
        if (!finite(latest)) return [1, clock, 0, 0];
        if (!bounded(clock) || !bounded(latest)) throw unsupported;
        math(['max', 'min', 'floor']);
        return [1, clock, 1, latest];
      });
      return words ? run(12, words, read => ({ value: read() })) : null;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      const owner = handle; handle = 0;
      if (owner) x.prediction_playout_drop(owner);
    },
  };
}
