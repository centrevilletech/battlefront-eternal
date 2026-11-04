// Publication policy and retained baselines on the shared simulation.
// Transport IDs and captured objects remain foreign handles; Rust owns the
// clocks, acknowledgements, queues, cadence and encoded baseline payloads.
import { bridgeFor } from '../sim/runtime.js';
import { WireHostListMirror, syncWireHostList, WireOutputMirror, syncWireOutput,
  putWireBaselineChoice, putWireHostOwn, putWireStaggerTake, putWireHostList, putWireCadences } from '../sim/gen/index.js';

const listOutput = { Mirror: WireHostListMirror, sync: syncWireHostList };
const byteOutput = { Mirror: WireOutputMirror, sync: syncWireOutput };
const METRICS = ['snapshots', 'datagrams', 'bytes', 'cosmeticsDropped', 'cosmeticsOwedDropped', 'cosmeticsResent', 'emptyBaselines', 'budgetSkips', 'ownOnly', 'staleSkips'];
export function createWireHost(instance, options) { return createWireHostBridge(bridgeFor(instance), options); }
export function createWireBudgetBridge(bridge, { bytesPerSecond, burstBytes, now }) {
  const x = bridge.x; let handle = x.wire_budget_create(bytesPerSecond, burstBytes, now);
  if (!handle) throw Error('invalid entity budget');
  function alive() { if (!handle) throw Error('entity budget disposed'); }
  function run(op, bytes, at) {
    alive();
    if (op !== 0 && (!Number.isSafeInteger(bytes) || bytes < 1 || bytes > 1100)) throw Error('invalid entity message size');
    if (!Number.isFinite(at)) throw Error('invalid budget clock');
    return x.wire_budget_run(handle, op, bytes, at);
  }
  const api = {
    available(at = performance.now()) { return run(0, 0, at); },
    take(bytes, at = performance.now()) { return !!run(1, bytes, at); },
    refund(bytes, at = performance.now()) { run(2, bytes, at); },
    dispose() { if (handle) { x.wire_budget_drop(handle); handle = 0; } },
  };
  ['rate', 'burst', 'tokens', 'at', 'offeredBytes', 'denied'].forEach((key, field) => Object.defineProperty(api, key, { enumerable: true,
    get() { alive(); return x.wire_budget_get(handle, field); },
    set(value) { alive(); x.wire_budget_set(handle, field, value); },
  }));
  return api;
}
export function createWireHostBridge(bridge, { local = false, budget = Infinity, staleAfterMs = Infinity, epoch = 1 } = {}) {
  const x = bridge.x, clients = new Map();
  let handle = x.wire_host_create(+local, budget, staleAfterMs, epoch), nextId = 1;
  const inputs = [putWireBaselineChoice, putWireHostOwn].map((put, op) => ({ put, words: x.wire_host_words(op) }));
  function alive() { if (!handle) throw Error('snapshot host disposed'); }
  function op(code, state = null, a = 0, b = 0) { alive(); return x.wire_host_op(handle, code, state?.nativeId ?? 0, a, b); }
  function list(ptr) { return bridge.callResult(listOutput, ptr).values; }
  function property(target, name, state, field, boolean = false) {
    Object.defineProperty(target, name, { enumerable: true,
      get() { alive(); const value = x.wire_host_get(handle, state.nativeId, field); return boolean ? !!value : value; },
      set(value) { alive(); x.wire_host_set(handle, state.nativeId, field, boolean ? +!!value : value); },
    });
  }
  return {
    get interval() { alive(); return x.wire_host_get(handle, 0, 0); },
    setLocalInterval(ticks) { alive(); x.wire_host_set(handle, 0, 0, ticks); },
    add(id, at) {
      if (clients.has(id)) throw Error('snapshot client already joined');
      const state = { nativeId: nextId++, metrics: {} };
      op(0, state, at);
      for (const [name, field, boolean] of [['started', 2, true], ['ready', 3, true], ['baselineTick', 4], ['ownSentAt', 5], ['heardAt', 6], ['ownSummariesDue', 7]]) property(state, name, state, field, boolean);
      METRICS.forEach((name, i) => property(state.metrics, name, state, 10 + i));
      clients.set(id, state); return state;
    },
    remove(id) { const state = clients.get(id); if (state) { op(1, state); clients.delete(id); } },
    due(tick) { return !!op(2, null, tick); },
    captured(tick) { op(3, null, tick); },
    acceptAck(state, ack) { return !!(ack && Number.isSafeInteger(ack.tick) && ack.epoch === epoch && op(4, state, ack.tick, ack.epoch)); },
    budgetUsed(state, at) { return op(5, state, at); },
    chargeBytes(state, at, bytes) { op(6, state, at, bytes); },
    eligible(state, at, snapshot) { return !!op(7, state, at, +!!snapshot); },
    delivery(state, at) { return op(8, state, at); },
    resync(state) { op(9, state); },
    heldAcks(state, tick) { alive(); return list(x.wire_host_held(handle, state.nativeId, tick)); },
    chooseSharedBaselines(tick, acks, max = 3) {
      alive(); bridge.callArgs(inputs[0], { tick, acks: acks.map(ticks => ({ ticks })), max });
      return list(x.wire_baselines());
    },
    prepareOwn(state, { tick, baselineTick, correction, summaries }) {
      alive(); bridge.callArgs(inputs[1], { tick, epoch, baselineTick, correction, summaries: [...summaries.values()] });
      const out = bridge.callResult(byteOutput, x.wire_host_own(handle, state.nativeId));
      if (out.error !== null) throw Error(out.error);
      return { correctionDelta: correction ? Uint8Array.from(out.parts[0].bytes) : null, summariesSection: Uint8Array.from(out.parts[1].bytes) };
    },
    staleClients(at, seconds) {
      alive(); const ids = new Set(list(x.wire_host_stale(handle, at, seconds)));
      return [...clients].filter(([, state]) => ids.has(state.nativeId)).map(([id]) => id);
    },
    dispose() { if (handle) { x.wire_host_drop(handle); handle = 0; clients.clear(); } },
  };
}

export function createWireStagger(instance, cadence) { return createWireStaggerBridge(bridgeFor(instance), cadence); }
export function createWireStaggerBridge(bridge, cadence) {
  const x = bridge.x, identities = new WeakMap();
  const inputs = [putWireStaggerTake, putWireHostList, putWireCadences].map((put, op) => ({ put, words: x.wire_stagger_words(op) }));
  let handle = x.wire_stagger_create(), nextId = 1, held = new Map();
  function alive() { if (!handle) throw Error('world field stagger disposed'); }
  if (cadence !== undefined) {
    bridge.callArgs(inputs[2], { values: Object.entries(cadence).filter(([, ticks]) => ticks != null).map(([schema, ticks]) => ({ schema: +schema, ticks })) });
    x.wire_stagger_cadence(handle);
  }
  function token(record) { let id = identities.get(record); if (id === undefined) { id = nextId++; identities.set(record, id); } return id; }
  function checkSchemas(tick, schemas, any) {
    alive(); bridge.callArgs(inputs[1], { values: [...schemas] }); return !!x.wire_stagger_schemas(handle, tick, +any);
  }
  const api = {
    take(tick, records, completeSchemas = new Set()) {
      alive(); const refs = new Map(held);
      const values = records.map(record => {
        const id = token(record);
        if (!refs.has(id)) refs.set(id, { record, field: { schema: record.schema, id: record.id, generation: record.generation, payload: record.payload } });
        return { token: id, field: refs.get(id).field };
      });
      bridge.callArgs(inputs[0], { tick, records: values, complete: [...completeSchemas] });
      const ids = bridge.callResult(listOutput, x.wire_stagger_take(handle)).values;
      held = new Map(ids.map(id => [id, refs.get(id)]));
    },
    update(tick, records, complete) { api.take(tick, records, complete); return api.values(); },
    values() { alive(); return [...held.values()].map(value => value.record); },
    world() { alive(); return new Map([...held.values()].map(({ field }) => [`${field.schema}:${field.id}`, field])); },
    held(tick, schema, id, stateTeam = null) {
      alive(); const state = stateTeam === null ? -1 : bridge.strings.id(stateTeam);
      const result = x.wire_stagger_held(handle, tick, schema, id, state);
      return result < 0 ? undefined : result === 0 ? null : held.get(result).record;
    },
    isFieldDue(tick, schema, id) { alive(); return !!x.wire_stagger_due(handle, tick, schema, id); },
    isDue(tick, key) { const [schema, id] = key.split(':').map(Number); return api.isFieldDue(tick, schema, id); },
    anyDue(tick, schemas) { return checkSchemas(tick, schemas, true); },
    schemaDue(tick, schemas) { return checkSchemas(tick, schemas, false); },
    dispose() { if (handle) { x.wire_stagger_drop(handle); handle = 0; held.clear(); } },
  };
  return api;
}
