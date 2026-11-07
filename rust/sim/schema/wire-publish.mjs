// Publication bytes are spans into an owned native arena. Layouts describe
// transport results and foreign facts; they never describe canonical bodies.
export default {
  records: {
    WireDynamicFact: { $layout: true, slot: 'u32', refGeneration: 'u32', type: 'u8', id: 'f64', generation: 'f64', launchTick: 'f64', definitionId: 'f64', ownerId: 'f64', ownerGeneration: 'f64', fireId: 'f64', channel: 'f64', terminal: 'bool' },
    WireSpan: { $layout: true, offset: 'u32', length: 'u32' },
    WirePublishedGroup: { $layout: true, baseline: 'f64', clients: 'list:u32', parts: 'list:rec:WireSpan', bytes: 'f64' },
    WirePublishedOwn: { $layout: true, client: 'u32', span: 'rec:WireSpan' },
    WirePublishedAck: { $layout: true, client: 'u32', generation: 'f64', ack: 'f64', ackTick: 'f64', discardedInput: 'f64', discardedActions: 'f64' },
    WirePublishedOutcome: { $layout: true, id: 'f64', status: 'rec:WireSpan', tick: 'f64' },
    WirePublishedEdges: { $layout: true, client: 'u32', epoch: 'f64', generation: 'f64', received: 'list:f64', results: 'list:rec:WirePublishedOutcome' },
    WirePublishedRecord: { $layout: true, id: 'f64', generation: 'f64', tick: 'f64', schema: 'f64', payload: 'rec:WireSpan' },
    WirePublishedReliable: { $layout: true, client: 'u32', records: 'list:u32' },
    WirePublishedFault: { $layout: true, client: 'u32', message: 'rec:WireSpan' },
    WirePublicationOutput: { $layout: true, epoch: 'f64', tick: 'f64', capturedAtUnixMs: 'f64', droppedTicks: 'f64', drained: 'bool',
      shared: 'list:rec:WirePublishedGroup', own: 'list:rec:WirePublishedOwn', acks: 'list:rec:WirePublishedAck', edges: 'list:rec:WirePublishedEdges',
      records: 'list:rec:WirePublishedRecord', reliable: 'list:rec:WirePublishedReliable', faults: 'list:rec:WirePublishedFault',
      created: 'list:rec:WireDynamicFact', retired: 'list:u32', bytes: 'u32', byteLength: 'u32', ringNewest: 'f64', encodes: 'u32' },
    WireJournalFact: { $layout: true, id: 'f64', generation: 'f64', tick: 'f64', schema: 'f64', payload: 'rec:WireSpan',
      hasPosition: 'bool', position: 'vec3', ownerTag: 'u8', owner: 'f64', speed: 'f64', life: 'f64' },
  },
};
