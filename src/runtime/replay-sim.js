import { bridgeFor } from '../sim/runtime.js';

const unsupported = Symbol('observable replay input');
const viewFields = ['mouseScale', 'invertMouse', 'firstPerson', 'aspect', 'viewTick', 'flightSettings', 'gamepad'];
const arrayPrototype = Array.prototype, objectPrototype = Object.prototype;
const arrayIterator = arrayPrototype[Symbol.iterator], push = arrayPrototype.push;
const defineProperty = Object.defineProperty, hasOwn = objectPrototype.hasOwnProperty;
const getPrototypeOf = Object.getPrototypeOf, getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
const getOwnPropertyNames = Object.getOwnPropertyNames, ownKeys = Reflect.ownKeys, apply = Reflect.apply;
const isArray = Array.isArray, isFinite = Number.isFinite, isInteger = Number.isInteger, number = Number, string = String;
const SetClass = Set, setAdd = Set.prototype.add, setDelete = Set.prototype.delete, setEach = Set.prototype.forEach;
const setSize = getOwnPropertyDescriptor(Set.prototype, 'size').get;
const iteratorPrototype = getPrototypeOf(apply(arrayIterator, [], []));
const iteratorNext = iteratorPrototype.next;
let nextSession = 1;

function append(array, value) {
  defineProperty(array, array.length, { __proto__: null, value, writable: true, enumerable: true, configurable: true });
}
function own(value, key) { return apply(hasOwn, value, [key]); }
function descriptorValue(found) { return found && own(found, 'value') ? found.value : undefined; }
function copy(values) {
  const result = [];
  for (let i = 0; i < values.length; i++) append(result, values[i]);
  return result;
}
function descriptor(value, key) {
  for (let at = value; at; at = getPrototypeOf(at)) {
    const found = getOwnPropertyDescriptor(at, key);
    if (found) return found;
  }
}
function data(value, key) {
  if (value === null || typeof value !== 'object' && typeof value !== 'function') throw unsupported;
  const found = descriptor(value, key);
  if (!found) return undefined;
  if (!own(found, 'value')) throw unsupported;
  return found.value;
}
function record(value) {
  if (value === null || typeof value !== 'object') throw unsupported;
  const prototype = getPrototypeOf(value);
  if (prototype !== objectPrototype && prototype !== null) throw unsupported;
  return value;
}
function plainArray(value, iterated = false) {
  if (!isArray(value) || getPrototypeOf(value) !== arrayPrototype) throw unsupported;
  if (iterated && (descriptorValue(descriptor(value, Symbol.iterator)) !== arrayIterator
    || descriptorValue(descriptor(iteratorPrototype, 'next')) !== iteratorNext || descriptor(iteratorPrototype, 'return'))) throw unsupported;
  return value.length;
}
function entry(array, index) {
  const found = getOwnPropertyDescriptor(array, string(index));
  if (!found || !own(found, 'value')) throw unsupported;
  return found.value;
}
function finite(value) { return typeof value === 'number' && isFinite(value); }
function stamp(value) { return finite(value) && value > -(2 ** 52) && value < 2 ** 52; }
function metadata(make) {
  try { return make(); }
  catch (error) { if (error === unsupported) return null; throw error; }
}
function indexKey(key) {
  const n = number(key);
  return isInteger(n) && n >= 0 && n < 0xffffffff && string(n) === key;
}
function arrayEnvironment() {
  if (descriptorValue(descriptor(arrayPrototype, 'push')) !== push) throw unsupported;
  // Fresh array pushes must not encounter an inherited indexed setter or a
  // non-writable entry. Native frames otherwise use the original push order.
  for (let at = arrayPrototype; at; at = getPrototypeOf(at)) {
    const keys = getOwnPropertyNames(at);
    for (let i = 0; i < keys.length; i++) if (indexKey(keys[i])) throw unsupported;
  }
}
function outputEnvironment(view) {
  arrayEnvironment();
  if (getPrototypeOf(view) !== objectPrototype) throw unsupported;
  for (let i = 0; i < viewFields.length; i++) {
    const found = descriptor(view, viewFields[i]);
    if (found && (!own(found, 'value') || !found.writable)) throw unsupported;
  }
}
function initial(pending, from, generation) {
  arrayEnvironment();
  if (!stamp(from) || !finite(generation)) throw unsupported;
  const length = plainArray(pending, true), words = [from, generation, length];
  for (let i = 0; i < length; i++) {
    const command = record(entry(pending, i)), life = data(command, 'generation');
    if (!finite(life)) throw unsupported;
    append(words, life);
    if (life === generation) {
      const tick = data(command, 'tick');
      if (!stamp(tick)) throw unsupported;
      append(words, 1); append(words, tick);
    } else { append(words, 0); append(words, 0); }
  }
  return words;
}
function frameInput(pending, generation, position, down, view, fields) {
  outputEnvironment(view);
  if (plainArray(fields, true) !== viewFields.length) throw unsupported;
  for (let i = 0; i < viewFields.length; i++) if (entry(fields, i) !== viewFields[i]) throw unsupported;
  const length = plainArray(pending), arena = [{ value: undefined }, { value: null }];
  const token = value => {
    if (value === undefined) return 0;
    if (value === null) return 1;
    const at = arena.length;
    // Slot objects prevent V8 from storing opaque numbers in a packed-double
    // arena, which can quiet a signaling NaN before a simple assignment.
    append(arena, { value }); return at;
  };
  const arrayToken = value => {
    // V8's original iterator/read/push path can quiet or canonicalize NaNs
    // differently for each source element kind. Keep those frames intact.
    if (value !== value) throw unsupported;
    return token(value);
  };
  const words = [length, token(down)], keys = ownKeys(view);
  append(words, keys.length);
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i], found = getOwnPropertyDescriptor(view, key);
    let field = -1;
    for (let j = 0; j < viewFields.length; j++) if (viewFields[j] === key) { field = j; break; }
    if (field < 0 || !found || !own(found, 'value') || !found.enumerable || !found.writable) throw unsupported;
    append(words, field); append(words, token(found.value));
  }
  const slotCountAt = words.length; append(words, 0);
  const values = collection => {
    const size = plainArray(collection, true); append(words, size);
    for (let i = 0; i < size; i++) append(words, arrayToken(entry(collection, i)));
  };
  for (let index = position.index; index < length; index++) {
    const command = record(entry(pending, index)), tick = data(command, 'tick');
    if (!stamp(tick)) throw unsupported;
    words[slotCountAt]++;
    if (tick > position.tick) { append(words, 0); append(words, tick); break; }
    const life = data(command, 'generation');
    if (!finite(life)) throw unsupported;
    if (life !== generation) { append(words, 1); append(words, tick); append(words, life); continue; }
    append(words, 2); append(words, tick); append(words, life); append(words, arrayToken(data(command, 'sequence')));
    values(data(command, 'actions'));
    const unsent = data(command, 'sent') === false; append(words, +unsent);
    if (unsent) continue;
    append(words, token(data(command, 'down')));
    const worldLook = data(command, 'worldLook'); values(worldLook ?? data(command, 'look'));
    const countAt = words.length; append(words, 0);
    for (let i = 0; i < viewFields.length; i++) {
      const value = data(command, viewFields[i]);
      if (value === undefined) continue;
      words[countAt]++; append(words, i); append(words, token(value));
    }
  }
  return { words, arena };
}
function positionFrom(read) { return { mine: !!read(), last: read(), index: read(), tick: read(), phase: read() }; }

export function createReplaySim(instance, options) { return createReplayBridge(bridgeFor(instance), options); }

// Every invocation owns a cursor. Closing the factory rejects new invocations;
// active ones retain their handles until the helper's finally calls finish().
export function createReplayBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, sessions = new SetClass(); let disposed = false, calls = 0;
  const live = () => { if (disposed) throw Error('replay bridge is disposed'); };
  function begin(pending, from, generation) {
    live(); const words = metadata(() => initial(pending, from, generation)); live();
    if (!words) return null;
    const serial = nextSession++;
    let handle = x.prediction_replay_create(), active = true, busy = false, dropAfter = false, position = null;
    const usable = () => { if (!active || !handle) throw Error('replay session is disposed'); };
    function emit(op, input, output, error) {
      if (trace) trace({ session: serial, op, input: copy(input), output: copy(output), error });
    }
    function run(op, values, unpack) {
      usable();
      if (busy) throw Error('replay session is already running');
      busy = true;
      try {
        const input = new Float64Array(values.length);
        for (let i = 0; i < values.length; i++) input[i] = values[i];
        const at = x.prediction_replay_room(handle, input.length);
        bridge.views(); bridge.f.set(input, at / 8);
        const ok = x.prediction_replay_run(handle, op); calls++;
        bridge.views();
        const output = [], start = x.prediction_replay_output(handle) / 8, length = x.prediction_replay_output_len(handle);
        for (let i = 0; i < length; i++) append(output, bridge.f[start + i]);
        const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.prediction_replay_error(handle), x.prediction_replay_error_len(handle)));
        let cursor = 0;
        const read = () => output[cursor++];
        const result = ok ? unpack(read) : null;
        emit(op, input, output, error);
        if (!ok) throw Error(error);
        return result;
      } finally { busy = false; }
    }
    const session = {
      get handle() { return handle; },
      get disposed() { return !handle; },
      get dropRequested() { return dropAfter; },
      get position() { return position && { ...position }; },
      frame(pending, down, view, fields = viewFields, buffers = { actions: [], look: [], sequences: [] }) {
        usable();
        if (busy) throw Error('replay session is already running');
        const input = metadata(() => frameInput(pending, generation, position, down, view, fields));
        usable();
        if (!input) return null;
        return run(1, input.words, read => {
          position = positionFrom(read);
          const last = !!read(), any = !!read(), down = input.arena[read()].value;
          const list = result => {
            const count = read();
            for (let i = 0; i < count; i++) result.push(input.arena[read()].value);
            return result;
          };
          const actions = list(buffers.actions), look = list(buffers.look), sequences = list(buffers.sequences), count = read();
          for (let i = 0; i < count; i++) view[viewFields[read()]] = input.arena[read()].value;
          const command = { tick: position.tick, generation, down, actions, look, any, last, ...view };
          return { index: position.index, down, command, sequences };
        });
      },
      advance() {
        return run(2, [], read => { position = positionFrom(read); return { ...position }; });
      },
      dispose() { if (active && handle) dropAfter = true; },
      finish(suppressTraceError = false) {
        if (!handle) return;
        active = false;
        const old = handle; handle = 0; apply(setDelete, sessions, [session]);
        x.prediction_replay_drop(old);
        try { if (trace) emit('drop', [], [], null); }
        catch (error) { if (!suppressTraceError) throw error; }
      },
    };
    apply(setAdd, sessions, [session]);
    try {
      if (trace) emit('create', [], [], null);
      run(0, words, read => { position = positionFrom(read); });
      return session;
    } catch (error) { session.finish(true); throw error; }
  }
  return {
    bridge, begin,
    get calls() { return calls; },
    get activeSessions() { return apply(setSize, sessions, []); },
    get active() { return apply(setSize, sessions, []); },
    get handles() { const result = []; apply(setEach, sessions, [session => append(result, session.handle)]); return result; },
    dispose() { if (disposed) return; disposed = true; apply(setEach, sessions, [session => session.dispose()]); },
  };
}
