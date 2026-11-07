// Immutable protocol identities are distinct from store slots and class ids.
// Resolve the maps at attachment; a publication reads numeric handles only.
export default {
  records: {
    WireStoreName: { $layout: true, name: 'str', id: 'u32' },
    WireStoreDefinition: { $layout: true, definition: 'u32', id: 'u32' },
    WireStorePeer: { $layout: true, id: 'u32', character: 'rec:F4Handle|null', mailbox: 'u32' },
    WireSpawnFacts: { $layout: true, valid: 'bool', pending: 'bool', pendingClass: 'f64', pendingPost: 'f64', auto: 'bool', autoTeam: 'f64', autoClass: 'f64', autoPost: 'f64' },
    WireOwnFacts: { $layout: true, held: 'Uint8Array(16)', heldValid: 'bool', view: 'rec:WiView', discarded: 'f64', reconAspect: 'f64', combatFirstPerson: 'bool', spawn: 'rec:WireSpawnFacts' },
    WirePublicationLink: { $layout: true, id: 'u32', discardedActions: 'f64', rtt: 'f64', hasKind: 'bool', relay: 'bool', hasFamily: 'bool', familySix: 'bool' },
    WireWorldFacts: { $layout: true, tick: 'f64', at: 'f64', unixMs: 'f64', droppedTicks: 'f64', links: 'list:rec:WirePublicationLink' },
    WireStoreSetup: { $layout: true,
      unbuilt: 'str', built: 'str', destroyed: 'str',
      classes: 'list:rec:WireStoreName', weaponNames: 'list:rec:WireStoreName', posts: 'list:rec:WireStoreName', weapons: 'list:rec:WireStoreDefinition',
      peers: 'list:rec:WireStorePeer', items: 'list:rec:WireStoreDefinition', remotes: 'list:rec:WireStoreDefinition', ordnance: 'list:rec:WireStoreDefinition',
      itemCapacity: 'u32', remoteCapacity: 'u32', entityCapacity: 'u32', recordCapacity: 'u32', seatCapacity: 'u32',
    },
    WireStoreRecords: { error: 'str|null', records: 'list:rec:WireRecord', allocations: 'f64', reallocations: 'f64' },
  },
};
