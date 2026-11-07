// The transport consumes one native publication. Record construction, rings,
// policy and packet encoding never cross through JS client/entity objects.
export function createWirePublisher(bridge, { runtime, source, stagger, shots, host, combat }) {
  const operation=bridge.publisher({runtime,source,stagger,shots,host,combat});
  const clients=new Map(),decoder=new TextDecoder();
  const copy=operation.copy.bind(operation),status=operation.status.bind(operation);
  // The generated exchange remains private. Transport rows and byte buffers
  // below own their values before the next publication reuses this mirror.
  let ringNewest = -1, encodes = 0;
  return {
    bind(id, state, actor) {
      operation.bind(state,actor);
      clients.set(state.nativeId, id);
    },
    remove(state) { clients.delete(state.nativeId); },
    get ringNewest() { return ringNewest; }, get encodes() { return encodes; },
    publish({ tick, at, unixMs, droppedTicks }) {
      // F5 still delivers ordered journal callbacks and decoded mailbox facts.
      // Their reserved input block is consumed in the same native publication.
      const result=operation.publish(tick,at,unixMs,droppedTicks);
      if (bridge.boundaryCounters) bridge.boundaryCounters.outputBytes = (bridge.boundaryCounters.outputBytes ?? 0) + result.byteLength;
      if (result.drained) { combat.dynamic?.adopt(result.created, bridge); combat.dynamic?.retire(result.retired); }
      ringNewest = result.ringNewest; encodes = result.encodes;
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
    dispose() { operation.close(); clients.clear(); },
  };
}
