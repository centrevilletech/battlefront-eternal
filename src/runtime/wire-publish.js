// The transport consumes one native publication. Record construction, rings,
// policy and packet encoding never cross through JS client/entity objects.
import { WirePublicationOutputMirror, syncWirePublicationOutput } from '../sim/gen/index.js';
const output = { Mirror: WirePublicationOutputMirror, sync: syncWirePublicationOutput };
export function createWirePublisher(bridge, { runtime, source, stagger, shots, host, combat }) {
  const x = bridge.x, clients = new Map(), decoder = new TextDecoder();
  let handle = x.wire_publish_create(runtime.nativeHandle, source.nativeHandle, stagger.nativeHandle, shots.nativeHandle, host.nativeHandle, combat.native.handle);
  if (!handle) throw Error('publication attachment failed');
  // The generated exchange remains private. Transport rows and byte buffers
  // below own their values before the next publication reuses this mirror.
  const exchange = new WirePublicationOutputMirror();
  let ringNewest = -1, encodes = 0;
  return {
    bind(id, state, actor) {
      if (!x.wire_publish_bind(handle, state.nativeId, source.peerId(actor), state.edges.handle)) throw Error('publication client attachment failed');
      clients.set(state.nativeId, id);
    },
    remove(state) { clients.delete(state.nativeId); },
    get ringNewest() { return ringNewest; }, get encodes() { return encodes; },
    publish({ tick, at, unixMs, droppedTicks }) {
      // F5 still delivers ordered journal callbacks and decoded mailbox facts.
      // Their reserved input block is consumed in the same native publication.
      source.writeWorldFacts(tick, { at, unixMs, droppedTicks, journal: combat });
      const result = bridge.callResult(output, x.wire_publish(handle), exchange);
      if (bridge.boundaryCounters) bridge.boundaryCounters.outputBytes = (bridge.boundaryCounters.outputBytes ?? 0) + result.byteLength;
      if (result.drained) { combat.pending.clear(); if (combat.dynamic) combat.dynamic.pending.clear(); combat.dynamic?.adopt(result.created, bridge); combat.dynamic?.retire(result.retired); }
      ringNewest = result.ringNewest; encodes = result.encodes;
      bridge.views();
      const bytes = bridge.u8, base = result.bytes;
      const copy = span => bytes.slice(base + span.offset, base + span.offset + span.length);
      const status = span => {
        let text = '';
        for (let at = base + span.offset, end = at + span.length; at < end; at += 2) text += String.fromCharCode(bytes[at] | bytes[at + 1] << 8);
        return text;
      };
      const { epoch, tick: serverTick, capturedAtUnixMs } = result;
      const shared = result.shared.map(group => ({ epoch, tick: serverTick, baselineTick: group.baseline, capturedAtUnixMs,
        clients: group.clients.map(id => clients.get(id)), parts: group.parts.map(span => copy(span).buffer), bytes: group.bytes }));
      const ownPackets = result.own.map(packet => ({ client: clients.get(packet.client), epoch, tick: serverTick, own: true, capturedAtUnixMs, buffer: copy(packet.span).buffer }));
      const inputAcks = result.acks.map(row => ({ client: clients.get(row.client), epoch, serverTick, generation: row.generation, ack: row.ack, ackTick: row.ackTick,
        dropped: result.droppedTicks, discardedInput: row.discardedInput, discardedActions: row.discardedActions, ackActions: 0 }));
      const edgeResults = result.edges.map(row => ({ client: clients.get(row.client), epoch: row.epoch, generation: row.generation,
        received: row.received.slice(), results: row.results.map(r => ({ id: r.id, status: status(r.status), tick: r.tick })) }));
      const records = result.records.map(row => ({ kind: 'combat', id: row.id, generation: row.generation, tick: row.tick, schema: row.schema, payload: copy(row.payload) }));
      const reliableEvents = result.reliable.map(row => ({ client: clients.get(row.client), tick: serverTick, records: row.records.map(i => records[i]) }));
      const faults = result.faults.map(row => ({ client: row.client ? clients.get(row.client) : null, error: Error(decoder.decode(copy(row.message))) }));
      return { shared, ownPackets, inputAcks, edgeResults, reliableEvents, diagnostics: [], faults };
    },
    dispose() { if (handle) { x.wire_publish_drop(handle); handle = 0; clients.clear(); } },
  };
}
