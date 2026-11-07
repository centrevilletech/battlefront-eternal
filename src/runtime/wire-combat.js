// Registry callbacks and decoded publication views stay at the boundary.
// The native producer owns payload bytes, event IDs and queue decisions.
import { bridgeFor } from '../sim/runtime.js';
import { WbResultMirror, WbSavedResultMirror, WbCommitLayout, WireJournalFactMirror, WireDynamicFactLayout, WireOutputMirror, putWbCommit, putWireDynamicFact, putWbCall, putWbEncode, syncWbResult, syncWbSavedResult, syncWireOutput } from '../sim/gen/index.js';

const output = { Mirror: WbResultMirror, sync: syncWbResult };
const bytesOutput = { Mirror: WireOutputMirror, sync: syncWireOutput };
const number = n => typeof n === 'number' ? n : NaN;
const numbers = v => Array.from(v, number);
const array = v => Array.isArray(v) ? numbers(v) : [];
const record = (r, schema, payload) => ({ kind: 'combat', id: r.id, generation: r.generation, tick: r.tick, schema, payload });

export class WireCombat {
  constructor({ instance = null, bridge = instance ? bridgeFor(instance) : null }) {
    this.bridge = bridge; this.handle = bridge.x.wire_combat_create();
    this.input = { put: putWbCall, words: bridge.x.wire_combat_words(0) };
    this.commitInput = { put: putWbCommit, words: WbCommitLayout.words };
    this.dynamicInput = { put: putWireDynamicFact, words: WireDynamicFactLayout.words };
    this.fact = new WireJournalFactMirror();
    this.encoding = { put: putWbEncode, words: bridge.x.wire_combat_words(1) };
    // These exchanges remain private; records copy their payload and queue
    // keys before another callback or operation can overwrite either mirror.
    this.exchange = new WbResultMirror(); this.encoded = new WireOutputMirror();
  }
  run(op, args = {}, into = null) {
    if (!this.handle) throw Error('combat runtime disposed');
    this.bridge.callArgs(this.input, { op, tick: 0, capacity: 0, key: 0, id: 0, schema: 0, expiryTick: 0, text: '', ...args });
    return this.bridge.callResult(output, this.bridge.x.wire_combat_run(this.handle), into);
  }
  encode(op, args) {
    if (!this.handle) throw Error('combat runtime disposed');
    this.bridge.callArgs(this.encoding, { op, id: 0, generation: 0, tick: 0, schema: 0, expiryTick: 0, definitionId: 0,
      ownerId: 0, ownerGeneration: 0, fireId: 0, channel: 0, launchTick: 0, executionTick: 0,
      position: [], direction: [], offset: [], velocity: [], quaternion: [], end: [], positionArray: false, directionArray: false, offsetArray: false,
      speed: 0, life: 0, merge: 0, surface: 0, targetId: 0, targetGeneration: 0, attackerId: 0, attackerGeneration: 0,
      team: 0, attackerTeam: 0, source: [], hasSource: false, wet: false, terminal: false, type: 0, ...args });
    const out = this.bridge.callResult(bytesOutput, this.bridge.x.wire_combat_encode(), this.encoded);
    if (out.error !== null) throw Error(out.error);
    return Uint8Array.from(out.parts[0].bytes);
  }
  bolt(r) {
    const { position, direction, offset } = r;
    const payload = this.encode(0, {
      ...Object.fromEntries(['id', 'generation', 'tick', 'schema', 'expiryTick', 'definitionId', 'ownerId', 'ownerGeneration', 'fireId',
        'channel', 'launchTick', 'executionTick', 'speed', 'life', 'merge'].map(k => [k, number(r[k])])),
      position: array(position), direction: array(direction), offset: array(offset), surface: Number(r.surface),
      positionArray: Array.isArray(position), directionArray: Array.isArray(direction), offsetArray: Array.isArray(offset),
    });
    return record(r, r.schema, payload);
  }
  feedback(r) {
    const e = r.event, source = e.source, schema = e.kind === 'damage' ? 3 : e.kind === 'death' ? 4 : 0;
    if (!schema) return record(r, schema, this.encode(1, { schema }));
    const hasSource = Array.isArray(source) && source.length === 3 && source.every(Number.isFinite);
    const payload = this.encode(1, { schema, expiryTick: number(r.tick + 18),
      targetId: number(e.target?.unit?.id ?? 0), targetGeneration: number(e.target?.generation ?? 0),
      attackerId: number(e.attacker?.unit?.id ?? 0), attackerGeneration: number(e.attacker?.generation ?? 0),
      position: numbers(e.position), team: Number(e.team ?? -1), attackerTeam: Number(e.attackerTeam ?? -1), hasSource, source: hasSource ? source : [] });
    return record(r, schema, payload);
  }
  explosion(r) {
    const payload = this.encode(2, { id: number(r.id), generation: number(r.generation), tick: number(r.tick),
      definitionId: number(r.definitionId), expiryTick: number(r.tick + 18), positionArray: Array.isArray(r.position), position: array(r.position), wet: !!r.wet });
    return record(r, 5, payload);
  }
  projectile(r) {
    const payload = this.encode(3, { ...Object.fromEntries(['id', 'generation', 'tick', 'definitionId', 'ownerId', 'ownerGeneration',
      'fireId', 'channel', 'launchTick', 'life'].map(k => [k, number(r[k])])),
      expiryTick: number(r.tick + 18), type: ['grenade', 'round', 'beam'].indexOf(r.type), terminal: !!r.terminal,
      ...(r.terminal ? {} : { position: numbers(r.position), velocity: numbers(r.velocity), quaternion: numbers(r.quaternion), end: numbers(r.end) }) });
    return record(r, r.terminal ? 7 : 6, payload);
  }
  emit(capture, record, decode) {
    const tick = capture.tick(), out = this.run(0, { tick, capacity: capture.maxRecords, id: record.id, schema: record.schema }, this.exchange);
    capture.prunedTick = tick;
    if (out.error === 'combat capture capacity exceeded') return false;
    if (out.error !== null) throw Error(out.error);
    const key = out.key, view = decode(record);
    this.run(1, { key, schema: record.schema, expiryTick: view.expiryTick }, this.exchange);
    const fact = this.fact;
    fact.id = record.id; fact.generation = record.generation; fact.tick = record.tick; fact.schema = record.schema;
    fact.payload.offset = 0; fact.payload.length = record.payload.length;
    fact.hasPosition = !!view.position; fact.position.set(view.position?.[0] ?? 0, view.position?.[1] ?? 0, view.position?.[2] ?? 0);
    fact.ownerTag = view.ownerId === undefined ? 0 : view.ownerId === null ? 1 : 2;
    fact.owner = typeof view.ownerId === 'number' ? view.ownerId : NaN;
    fact.speed = typeof view.speed === 'number' ? view.speed : NaN; fact.life = typeof view.life === 'number' ? view.life : NaN;
    this.bridge.callArgs(this.commitInput, { key, fact, payload: record.payload });
    return !!this.bridge.x.wire_combat_commit(this.handle);
  }
  saved(decode, drain = false) {
    if (!this.handle) throw Error('combat runtime disposed');
    const rows = this.bridge.callResult({ Mirror: WbSavedResultMirror, sync: syncWbSavedResult }, this.bridge.x.wire_combat_saved(this.handle, Number(drain))).records;
    const values = new Map();
    for (const row of rows) {
      const record = { kind: 'combat', id: row.fact.id, generation: row.fact.generation, tick: row.fact.tick, schema: row.fact.schema, payload: Uint8Array.from(row.payload) };
      values.set(row.key, { record, view: decode(record), expiresTick: row.expiresTick, retention: [3,4,5,8].includes(record.schema) ? 'cosmetic' : 'projectile' });
    }
    return values;
  }
  dynamic(entry) {
    this.bridge.callArgs(this.dynamicInput, { ...entry, type: entry.nativeType });
    return !!this.bridge.x.wire_combat_dynamic(this.handle);
  }
  drain(capture, decode) { return [...this.saved(decode, true).values()]; }
  get nextId() { if (!this.handle) throw Error('combat runtime disposed'); return this.bridge.x.wire_combat_next_id(this.handle); }
  set nextId(id) { if (!this.handle) throw Error('combat runtime disposed'); this.bridge.x.wire_combat_set_next_id(this.handle, id); }
  fault(error) { this.run(6, { text: String(error?.message ?? error) }); }
  drainFaults() { return this.run(7).faults; }
  get faults() { return this.run(8).faults; }
  dispose() { if (this.handle) { this.bridge.x.wire_combat_drop(this.handle); this.handle = 0; } }
}
