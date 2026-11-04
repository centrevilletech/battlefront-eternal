// Combat producers transfer samples by value. Native queue operations finish
// before the adapter decodes a view or invokes a simulation callback.
export default {
  records: {
    WbEncode: {
      op: 'f64', id: 'f64', generation: 'f64', tick: 'f64', schema: 'f64', expiryTick: 'f64', definitionId: 'f64',
      ownerId: 'f64', ownerGeneration: 'f64', fireId: 'f64', channel: 'f64', launchTick: 'f64', executionTick: 'f64',
      position: 'list:f64', direction: 'list:f64', offset: 'list:f64', velocity: 'list:f64', quaternion: 'list:f64', end: 'list:f64',
      positionArray: 'bool', directionArray: 'bool', offsetArray: 'bool',
      speed: 'f64', life: 'f64', merge: 'f64', surface: 'f64',
      targetId: 'f64', targetGeneration: 'f64', attackerId: 'f64', attackerGeneration: 'f64',
      team: 'f64', attackerTeam: 'f64', source: 'list:f64', hasSource: 'bool', wet: 'bool', terminal: 'bool', type: 'f64',
    },
    WbCall: {
      op: 'f64', tick: 'f64', capacity: 'f64', key: 'f64', id: 'f64', schema: 'f64', expiryTick: 'f64', text: 'str',
    },
    WbResult: { error: 'str|null', key: 'f64', ids: 'list:f64', nextId: 'f64', faults: 'list:str' },
  },
};
