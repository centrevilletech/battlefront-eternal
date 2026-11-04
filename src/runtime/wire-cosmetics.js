// Cosmetic bytes and delivery decisions on the world's shared simulation.
// The host supplies foreign values and keeps transport ownership; each
// outbox owns its native queues and buffers until explicitly disposed.
import { bridgeFor } from '../sim/runtime.js';
import { DEFAULT_SNAPSHOT_BOUNDS } from './snapshot-codec.js';

const KINDS = ['launch', 'terminal', 'sample', 'explosion', 'presentation'];
const number = value => typeof value === 'number' ? value : NaN;
const vec = (words, value, object = false) => {
  words.push(value ? 1 : 0);
  if (!value) words.push(0, 0, 0);
  else if (object) words.push(value.x, value.y, value.z);
  else words.push(value[0], value[1], value[2]);
};
const owner = (words, value) => words.push(value === undefined ? 0 : value === null ? 1 : 2, number(value));
const viewWords = (words, view) => {
  words.push(number(view?.schema)); vec(words, view?.position); vec(words, view?.direction);
  owner(words, view?.ownerId); words.push(number(view?.speed), number(view?.life));
};
const string = (words, value) => { words.push(value.length); for (let i = 0; i < value.length; i++) words.push(value.charCodeAt(i)); };
function stringAt(words, cursor) {
  const n = words[cursor.at++]; let value = '';
  for (let i = 0; i < n; i++) value += String.fromCharCode(words[cursor.at++]);
  return value;
}

export function createWireCosmetics(instance, options) { return createWireCosmeticsBridge(bridgeFor(instance), options); }
export function createWireCosmeticsBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, sessions = new Set(), identities = new WeakMap();
  let nextIdentity = 1, nextSession = 1, disposed = false;
  const identity = event => {
    let id = identities.get(event);
    if (id === undefined) { id = nextIdentity++; identities.set(event, id); }
    return id;
  };
  function session() {
    if (disposed) throw Error('cosmetic encoder disposed');
    let handle = x.wire_cosmetics_create(); const id = nextSession++;
    const object = {
      run(op, words = [], payload = new Uint8Array()) {
        if (!handle) throw Error('cosmetic outbox disposed');
        // All callbacks run after output has been copied. A tracer may
        // reenter this encoder without replacing the outer call's result.
        const wp = x.wire_cosmetics_room(handle, words.length);
        const bp = x.wire_cosmetics_payload(handle, payload.length);
        bridge.views(); bridge.f.set(words, wp / 8); bridge.u8.set(payload, bp);
        const failed = x.wire_cosmetics_run(handle, op);
        const wo = x.wire_cosmetics_words(handle), wn = x.wire_cosmetics_word_count(handle);
        const bo = x.wire_cosmetics_bytes(handle), bn = x.wire_cosmetics_byte_count(handle);
        bridge.views();
        const result = { words: bridge.f.slice(wo / 8, wo / 8 + wn), bytes: bridge.u8.slice(bo, bo + bn) };
        let error = null;
        if (failed) {
          const ep = x.wire_cosmetics_error(handle), en = x.wire_cosmetics_error_len(handle);
          bridge.views(); error = new TextDecoder().decode(bridge.u8.subarray(ep, ep + en));
        }
        trace?.({ session: id, op, words: Float64Array.from(words), payload: Uint8Array.from(payload), result, error });
        if (error) throw Error(error);
        return result;
      },
      metric(index) { if (!handle) throw Error('cosmetic outbox disposed'); return x.wire_cosmetics_metric(handle, index); },
      dispose() { if (handle) { x.wire_cosmetics_drop(handle); handle = 0; } sessions.delete(object); },
    };
    sessions.add(object); return object;
  }
  const codec = session();
  function encodeCosmeticEvent(record, bounds = DEFAULT_SNAPSHOT_BOUNDS, view = null) {
    const words = [number(record.id), number(record.generation), number(record.tick), number(record.schema), +(record.kind === 'combat'),
      bounds.xMin, bounds.xExtent, bounds.yMin, bounds.yExtent, bounds.zMin, bounds.zExtent,
      +(view !== null && view !== undefined && (record.schema === 1 || record.schema === 2))];
    if (words[11]) {
      viewWords(words, view);
      words.push(number(view.expiryTick), number(view.definitionId), number(view.ownerGeneration), number(view.fireId), number(view.launchTick), number(view.executionTick), number(view.channel), number(view.surface), number(view.merge));
      words.push(view.offset?.length ?? 0);
      for (const n of view.offset ?? []) words.push(number(n));
    }
    return codec.run(0, words, record.payload).bytes;
  }
  function cosmeticPriority(view, position, ownerId, range = 300) {
    const words = []; viewWords(words, view); vec(words, position, true); owner(words, ownerId); words.push(range);
    const out = codec.run(1, words).words;
    return out[0] ? out[1] : null;
  }
  function cosmeticEvent(record, view, bytes) {
    const out = codec.run(2, [record.schema, record.id, record.tick, number(view?.executionTick), number(view?.life)]).words;
    const cursor = { at: 5 };
    return { record, view, bytes, key: stringAt(out, cursor), kind: KINDS[out[0]], projectile: out[1] ? out[2] : null, must: !!out[3], expires: out[4] };
  }
  function packEncodedCosmeticEvents(encoded, budgetBytes) {
    const words = [budgetBytes, encoded.length], chunks = []; let size = 0;
    for (const { bytes } of encoded) { words.push(size, bytes.length); chunks.push(bytes); size += bytes.length; }
    const payload = new Uint8Array(size); let at = 0;
    for (const bytes of chunks) { payload.set(bytes, at); at += bytes.length; }
    const out = codec.run(9, words, payload), f = out.words; let i = 0;
    const packed = [], dropped = [], count = f[i++];
    for (let n = 0; n < count; n++) packed.push(encoded[f[i++]].record);
    const dropCount = f[i++];
    for (let n = 0; n < dropCount; n++) dropped.push(encoded[f[i++]].record);
    return { bytes: out.bytes, packed, dropped };
  }
  function createOutbox({ range = 300 } = {}) {
    const state = session(); state.run(3, [range]);
    return {
      range,
      get dropped() { return state.metric(0); },
      get owedDropped() { return state.metric(1); },
      get resent() { return state.metric(2); },
      get prunedTick() { return state.metric(3); },
      get size() { return state.metric(4); },
      offer(events, position, unitId) {
        const words = [], chunks = []; let size = 0;
        vec(words, position, true); owner(words, unitId); words.push(events.length);
        for (const event of events) {
          words.push(identity(event)); string(words, event.key);
          words.push(event.record.schema, KINDS.indexOf(event.kind), +(event.projectile !== null), event.projectile ?? 0, +!!event.must, event.expires, size, event.bytes.length);
          viewWords(words, event.view); chunks.push(event.bytes); size += event.bytes.length;
        }
        const payload = new Uint8Array(size); let at = 0;
        for (const bytes of chunks) { payload.set(bytes, at); at += bytes.length; }
        state.run(4, words, payload);
      },
      pack(tick, firstBudget, secondBudget, confirmable, spare = Infinity) {
        const out = state.run(5, [tick, firstBudget, secondBudget, +!!confirmable, spare]), first = out.words[0];
        return { first: first ? out.bytes.slice(0, first) : null, second: out.words[1] ? out.bytes.slice(first) : null };
      },
      acknowledge(tick) { state.run(6, [tick]); },
      resend() { state.run(7); },
      inspect() {
        const f = state.run(8).words, c = { at: 4 };
        const entries = [], count = f[c.at++];
        for (let i = 0; i < count; i++) {
          const key = stringAt(f, c);
          entries.push([key, { identity: f[c.at++], d2: f[c.at++], rank: f[c.at++], sentTick: f[c.at++] }]);
        }
        const inflight = [], sentCount = f[c.at++];
        for (let i = 0; i < sentCount; i++) {
          const tick = f[c.at++], n = f[c.at++], events = [];
          for (let j = 0; j < n; j++) events.push(f[c.at++]);
          inflight.push({ tick, events });
        }
        const projectiles = [], projectileCount = f[c.at++];
        for (let i = 0; i < projectileCount; i++) projectiles.push([f[c.at++], { expires: f[c.at++], confirmed: !!f[c.at++] }]);
        return { dropped: f[0], owedDropped: f[1], resent: f[2], prunedTick: f[3], entries, inflight, projectiles };
      },
      dispose() { state.dispose(); },
    };
  }
  return { encodeCosmeticEvent, cosmeticEvent, cosmeticPriority, packEncodedCosmeticEvents, createOutbox, identity,
    dispose() { if (disposed) return; disposed = true; for (const s of [...sessions]) s.dispose(); } };
}
