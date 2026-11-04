import { bridgeFor } from '../sim/runtime.js';

const unsupported = Symbol('observable reconciliation state');
const typed = Object.getOwnPropertyDescriptors(Object.getPrototypeOf(Float64Array.prototype));
const floatSlice = Float64Array.prototype.slice, floatSort = Float64Array.prototype.sort;
const floatSpecies = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Float64Array), Symbol.species);
const arrayFilter = Array.prototype.filter, arraySlice = Array.prototype.slice;
const arraySpecies = Object.getOwnPropertyDescriptor(Array, Symbol.species);
const leadResetFields = ['gapAt', 'gapCount', 'arrivedAt', 'arrivedTick', 'latest', 'lead', 'late', 'worstGap', 'extra', 'cooldown', 'calm', 'measuredKey', 'capped'];

function descriptor(value, key) {
  for (let at = value; at; at = Object.getPrototypeOf(at)) {
    const found = Object.getOwnPropertyDescriptor(at, key);
    if (found) return found;
  }
}
function object(value) { return value !== null && (typeof value === 'object' || typeof value === 'function'); }
function data(value, key) {
  if (!object(value)) throw unsupported;
  const found = descriptor(value, key);
  if (!found) return undefined;
  if (!('value' in found)) throw unsupported;
  return found.value;
}
function writable(value, keys) {
  if (!object(value)) throw unsupported;
  for (const key of keys) {
    const own = Object.getOwnPropertyDescriptor(value, key), found = own || descriptor(value, key);
    if (found && (!('value' in found) || !found.writable) || !own && !Object.isExtensible(value)) throw unsupported;
  }
}
function numbers(value, keys, write = false) {
  if (write) writable(value, keys);
  return keys.map(key => {
    const n = data(value, key);
    if (typeof n !== 'number') throw unsupported;
    return n;
  });
}
function numeric(...values) { return values.every(value => typeof value === 'number'); }
function optionalNumber(value) {
  if (value !== null && typeof value !== 'number') throw unsupported;
  return value === null ? [0, 0] : [1, value];
}
function metadata(make) {
  try { return make(); }
  catch (error) { if (error === unsupported) return null; throw error; }
}
function species(value, expected) {
  const found = descriptor(value, Symbol.species);
  return found && found.get === expected.get && found.set === expected.set && !('value' in found);
}
function ring(value, memory, sorted = false) {
  if (!(value instanceof Float64Array) || Object.getPrototypeOf(value) !== Float64Array.prototype
    || Reflect.apply(typed[Symbol.toStringTag].get, value, []) !== 'Float64Array') throw unsupported;
  for (const key of ['length', 'buffer', 'byteOffset', 'byteLength']) {
    const found = descriptor(value, key), expected = typed[key];
    if (!found || found.get !== expected.get || found.set !== expected.set || 'value' in found) throw unsupported;
  }
  if (value.buffer === memory.buffer || typeof SharedArrayBuffer === 'function' && value.buffer instanceof SharedArrayBuffer) throw unsupported;
  if (sorted && (descriptor(value, 'slice')?.value !== floatSlice || descriptor(value, 'sort')?.value !== floatSort
    || descriptor(value, 'constructor')?.value !== Float64Array || !species(Float64Array, floatSpecies))) throw unsupported;
  const copy = new Array(value.length);
  for (let i = 0; i < copy.length; i++) copy[i] = value[i];
  return { value, copy };
}
function counter(at, count, length) {
  if (!Number.isSafeInteger(at) || !Number.isSafeInteger(count) || at < 0 || count < 0 || count > length
    || at >= length && !(at === 0 && length === 0)) throw unsupported;
}
function commands(value, sliced = false) {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype
    || descriptor(value, 'filter')?.value !== arrayFilter || descriptor(value, 'constructor')?.value !== Array
    || !species(Array, arraySpecies) || sliced && descriptor(value, 'slice')?.value !== arraySlice) throw unsupported;
  const copy = new Array(value.length);
  for (let i = 0; i < copy.length; i++) {
    const item = Object.getOwnPropertyDescriptor(value, String(i));
    if (!item || !('value' in item)) throw unsupported;
    copy[i] = item.value;
  }
  return copy;
}
function reader(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); let at = 0;
  return () => { const value = view.getFloat64(at, true); at += 8; return value; };
}

export function createReconcileSim(instance, options) { return createReconcileBridge(bridgeFor(instance), options); }

// The bridge owns one reusable parser. Models, checkpoints and command objects
// stay with their caller; every native phase copies its result before callbacks.
export function createReconcileBridge(bridge, { trace = null } = {}) {
  const x = bridge.x; let handle = 0, calls = 0, disposed = false;
  const live = () => { if (disposed) throw Error('reconciliation bridge is disposed'); };
  function prepare(make) { live(); const value = metadata(make); live(); return value; }
  function run(op, words, unpack) {
    live();
    const input = Float64Array.from(words);
    if (!handle) handle = x.prediction_reconcile_create();
    const at = x.prediction_reconcile_room(handle, input.length);
    bridge.views(); bridge.f.set(input, at / 8);
    const ok = x.prediction_reconcile_run(handle, op); calls++;
    bridge.views();
    const output = new Uint8Array(x.memory.buffer, x.prediction_reconcile_output(handle), x.prediction_reconcile_output_len(handle) * 8).slice();
    const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.prediction_reconcile_error(handle), x.prediction_reconcile_error_len(handle)));
    const result = ok ? unpack(reader(output)) : null;
    trace?.({ op, input: Array.from(input), output: Array.from(new Float64Array(output.buffer)), error });
    if (!ok) throw Error(error);
    return result;
  }
  function selected(op, input, items) {
    return run(op, input, read => Array.from({ length: read() }, () => items[read()]));
  }
  return {
    bridge,
    get calls() { return calls; },
    get handle() { return handle; },
    estimatorReset(value) {
      if (!prepare(() => { writable(value, ['at', 'count', 'offset', 'dropped']); return true; })) return false;
      return run(0, [], read => {
        value.at = read(); value.count = read();
        const offset = read(), n = read(); value.offset = offset ? n : null;
        const dropped = read(), d = read(); value.dropped = dropped ? d : null;
        return true;
      });
    },
    estimatorArrive(value, tick, now, dropped) {
      const state = prepare(() => {
        if (!numeric(tick, now, dropped)) throw unsupported;
        const [hz, window, at, count] = numbers(value, ['hz', 'window', 'at', 'count']);
        const offsets = ring(data(value, 'offsets'), x.memory);
        if (window !== offsets.copy.length) throw unsupported;
        counter(at, count, offsets.copy.length);
        writable(value, ['at', 'count', 'offset', 'dropped']);
        return { offsets, words: [hz, window, at, count, ...optionalNumber(data(value, 'offset')), ...optionalNumber(data(value, 'dropped')), tick, now, dropped, offsets.copy.length, ...offsets.copy] };
      });
      if (!state) return false;
      return run(1, state.words, read => {
        const at = read(), count = read(), present = read(), offset = read(), hasDropped = read(), dropped = read(), index = read(), entry = read();
        value.dropped = hasDropped ? dropped : null;
        if (index >= 0) state.offsets.value[index] = entry;
        value.at = at; value.count = count; value.offset = present ? offset : null;
        return true;
      });
    },
    estimatorEstimate(value, now) {
      const state = prepare(() => {
        if (!numeric(now)) throw unsupported;
        return [numbers(value, ['hz'])[0], ...optionalNumber(data(value, 'offset')), now];
      });
      return state ? run(2, state, read => { const present = read(), value = read(); return { value: present ? value : null }; }) : null;
    },
    leadReset(value) {
      const state = prepare(() => { writable(value, leadResetFields); return numbers(value, ['margin']); });
      if (!state) return false;
      return run(3, state, read => {
        value.gapAt = read(); value.gapCount = read(); const present = read(), at = read(); value.arrivedAt = present ? at : null;
        for (const key of ['arrivedTick', 'latest', 'lead', 'late', 'worstGap', 'extra', 'cooldown', 'calm', 'measuredKey']) value[key] = read();
        value.capped = !!read(); return true;
      });
    },
    leadArrive(value, tick, now) {
      const state = prepare(() => {
        if (!numeric(tick, now)) throw unsupported;
        const [hz, latest, arrivedTick, at, count] = numbers(value, ['hz', 'latest', 'arrivedTick', 'gapAt', 'gapCount']);
        const gaps = ring(data(value, 'gaps'), x.memory); counter(at, count, gaps.copy.length);
        writable(value, ['latest', 'arrivedAt', 'arrivedTick', 'gapAt', 'gapCount']);
        return { gaps, words: [hz, latest, ...optionalNumber(data(value, 'arrivedAt')), arrivedTick, at, count, tick, now, gaps.copy.length, ...gaps.copy] };
      });
      if (!state) return false;
      return run(4, state.words, read => {
        const latest = read(), present = read(), arrivedAt = read(), arrivedTick = read(), at = read(), count = read(), index = read(), entry = read();
        value.latest = latest;
        if (index >= 0) state.gaps.value[index] = entry;
        value.gapAt = at; value.gapCount = count; value.arrivedAt = present ? arrivedAt : null; value.arrivedTick = arrivedTick;
        return true;
      });
    },
    acknowledge(value, late, elapsed, capped) {
      const state = prepare(() => {
        if (!numeric(late, elapsed)) throw unsupported;
        const [maxLead, cooldownAcks, calmAcks] = numbers(value, ['maxLead', 'cooldownAcks', 'calmAcks']);
        const fields = numbers(value, ['late', 'extra', 'cooldown', 'calm'], true);
        return [maxLead, cooldownAcks, calmAcks, +!!data(value, 'recover'), ...fields, late, elapsed, +!!capped];
      });
      if (!state) return false;
      return run(5, state, read => { for (const key of ['late', 'extra', 'cooldown', 'calm']) value[key] = read(); return true; });
    },
    measureKey(value) {
      const state = prepare(() => { writable(value, ['measuredKey']); return numbers(value, ['gapCount', 'gapAt', 'measuredKey']); });
      return state ? run(6, state, read => {
        const changed = !!read(), key = read();
        if (changed) value.measuredKey = key;
        return { changed, key };
      }) : null;
    },
    measure(value) {
      const state = prepare(() => {
        const [count] = numbers(value, ['gapCount']), gaps = ring(data(value, 'gaps'), x.memory, true);
        if (!Number.isSafeInteger(count) || count < 0 || count > gaps.copy.length) throw unsupported;
        // The native numeric sort does not promise V8's payload order among
        // distinct NaNs. Keep that slice/sort at its current JavaScript phase.
        for (let i = 0; i < count; i++) if (Number.isNaN(gaps.copy[i])) throw unsupported;
        return [count, gaps.copy.length, ...gaps.copy];
      });
      return state ? run(7, state, read => ({ gap: read() })) : null;
    },
    target(value, estimate, latest, roundTrip) {
      const state = prepare(() => {
        if (!numeric(latest, roundTrip)) throw unsupported;
        writable(value, ['capped', 'lead']);
        return [...numbers(value, ['hz', 'margin', 'maxLead', 'worstGap', 'extra']), +!!data(value, 'extrapolate'), ...optionalNumber(estimate), latest, roundTrip];
      });
      return state ? run(8, state, read => { const result = read(); value.capped = !!read(); value.lead = read(); return { value: result }; }) : null;
    },
    smootherReset(value) {
      if (!prepare(() => { writable(value, ['x', 'y', 'z']); return true; })) return false;
      return run(9, [], read => { const x = read(), y = read(), z = read(); value.z = z; value.y = y; value.x = x; return true; });
    },
    note(value, ox, oy, oz, nx, ny, nz) {
      const state = prepare(() => {
        if (!numeric(ox, oy, oz, nx, ny, nz)) throw unsupported;
        return [...numbers(value, ['x', 'y', 'z', 'count', 'worst'], true), ...numbers(value, ['snap']), ox, oy, oz, nx, ny, nz];
      });
      if (!state) return null;
      return run(10, state, read => {
        const size = read(), changed = !!read(), x = read(), y = read(), z = read(), count = read(), worst = read(), reset = !!read();
        if (changed) { value.count = count; value.worst = worst; value.x = x; value.y = y; value.z = z; }
        return { size, reset };
      });
    },
    decay(value, dt) {
      const state = prepare(() => {
        if (!numeric(dt)) throw unsupported;
        return [...numbers(value, ['x', 'y', 'z'], true), ...numbers(value, ['tau']), dt];
      });
      return state ? run(11, state, read => { value.x = read(); value.y = read(); value.z = read(); return { reset: !!read() }; }) : null;
    },
    length(value) {
      const state = prepare(() => numbers(value, ['x', 'y', 'z']));
      return state ? run(12, state, read => ({ value: read() })) : null;
    },
    windowDecision(value, checkpoint, sameLife, authorityTick) {
      const state = prepare(() => {
        if (!sameLife) return [0, 0, 0, 0, 0];
        if (!numeric(authorityTick)) throw unsupported;
        return [1, authorityTick, ...numbers(checkpoint, ['authorityTick', 'tick']), ...numbers(value, ['limit'])];
      });
      return state ? run(13, state, read => ({ mode: read() })) : null;
    },
    filterCommands(pending, checkpoint) {
      const state = prepare(() => {
        const items = commands(pending), [sequence] = numbers(checkpoint, ['sequence']);
        return { items, words: [sequence, items.length, ...items.map(command => numbers(command, ['sequence'])[0])] };
      });
      return state ? selected(14, state.words, state.items) : null;
    },
    retainCommands(pending, tick, generation, epoch, limit) {
      const state = prepare(() => {
        if (!numeric(tick, generation, epoch, limit)) throw unsupported;
        const items = commands(pending, true), words = [tick, generation, epoch, limit, items.length];
        for (const command of items) words.push(...numbers(command, ['generation', 'epoch', 'tick']));
        return { items, words };
      });
      return state ? selected(15, state.words, state.items) : null;
    },
    dispose() { if (disposed) return; disposed = true; if (handle) { x.prediction_reconcile_drop(handle); handle = 0; } },
  };
}
