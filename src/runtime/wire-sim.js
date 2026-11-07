// Snapshot encoders on the battle's shared module. The generated boundary
// accepts owned records and returns bytes independent of module memory.
import { bridgeFor } from '../sim/runtime.js';
import {
  WireEntryMirror, WireOutputMirror, putWireEncode, putWireDelta, putWireWorld, putWireOwn,
  putWireCapture, syncWireOutput,
} from '../sim/gen/index.js';
import { DEFAULT_SNAPSHOT_BOUNDS } from './snapshot-codec.js';

const entryDefaults = new WireEntryMirror();
const entry = value => ({ ...entryDefaults, seatSlot: -1, ...value });
const fields = world => world ? [...world.values()] : null;
const snapshot = s => s ? { tick: s.tick, epoch: s.epoch, slots: [...s.slots].map(([slot, e]) => ({ slot, entry: entry(e) })), world: fields(s.world) } : null;
const recordKinds = new Set(['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote', 'mount', 'seat', 'tombstone']);
const inputPuts = [putWireEncode, putWireDelta, putWireWorld, putWireOwn, putWireCapture];
const output = { Mirror: WireOutputMirror, sync: syncWireOutput };
export function createWireSim(instance, options) { return createWireBridge(bridgeFor(instance), options); }
export function createWireBridge(bridge, { trace = null } = {}) {
  const x = bridge.x;
  const inputs = inputPuts.map((put, op) => ({ put, words: x.wire_words(op) }));
  function read(ptr) {
    const out = bridge.callResult(output, ptr);
    if (out.error !== null) throw Error(out.error);
    return out.parts.map(p => Uint8Array.from(p.bytes));
  }
  function call(op, args) {
    bridge.callArgs(inputs[op], args);
    // Tests can save the exact generated input block for the native twin.
    if (trace) { trace(op, bridge); bridge.callArgs(inputs[op], args); }
    return read(x.wire_encode(op));
  }
  let handle = 0;
  function alive() { if (!handle) throw Error('snapshot runtime disposed'); }
  return {
    bridge,
    encodeEntities(current, baseline, { interval = 2 } = {}) { return call(0, { current: snapshot(current), baseline: snapshot(baseline), interval }); },
    encodeWordDelta(current, baseline = null) { return call(1, { current, baseline })[0]; },
    encodeWorldSection(current, baseline = null) { return call(2, { current: fields(current), baseline: fields(baseline) })[0]; },
    encodeOwnDatagram({ epoch, tick, interval = 2, baselineTick = 0, ackInput = 0, correction = null, baselinePayload = null,
      correctionDelta = null, summaries = null, baselineSummaries = null, summariesSection = null, cosmeticBytes = null,
      second = false, ownParts = second ? 2 : 1 }) {
      return call(3, { epoch, tick, interval, baselineTick, ackInput, correction, baselinePayload, correctionDelta,
        summaries: fields(summaries), baselineSummaries: fields(baselineSummaries), summariesSection, cosmeticBytes, second, ownParts })[0];
    },
    // State is allocated only for a host, not for users of the pure codec.
    start() { if (!handle) handle = x.wire_create(); },
    get nativeHandle() { if (!handle) throw Error('snapshot runtime disposed'); return handle; },
    bindStore(source) {
      alive();
      if (source.bridge !== bridge || !x.wire_bind_store(handle, source.nativeHandle)) throw Error('canonical snapshot ring attachment failed');
    },
    capture({ tick, epoch = 1, records, world, bounds = DEFAULT_SNAPSHOT_BOUNDS }) {
      alive();
      bridge.callArgs(inputs[4], { tick, epoch, records: records.filter(r => recordKinds.has(r.kind)), world: fields(world) ?? [], bounds });
      read(x.wire_capture(handle));
      return { tick, epoch };
    },
    encodeHeld(current, baseline, { interval = 2 } = {}) {
      alive(); return read(x.wire_held(handle, current.tick, current.epoch, baseline?.tick ?? 0, interval));
    },
    dispose() { if (handle) { x.wire_drop(handle); handle = 0; } },
  };
}
