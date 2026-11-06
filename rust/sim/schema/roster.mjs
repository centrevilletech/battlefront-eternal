export default {
  enums: {
    RoOperation: ['create', 'reserve', 'release', 'switchTeam', 'counts'],
    RoActionOp: ['attach', 'detach', 'retire', 'botName', 'insert', 'newHuman', 'removeBody'],
  },
  records: {
    RoPlayers: { capacity: 'f64', reserved: 'list:ref:character' },
    RoInput: { op: 'enum:RoOperation', classic: 'bool', players: 'rec:RoPlayers', armySize: 'f64|null', hasAi: 'bool', local: 'ref:character|null', character: 'ref:character|null', team: 'f64', playerId: 'union:PlayerId', name: 'str' },
    RoCount: { team: 'f64', humans: 'f64', total: 'f64|null' },
    RoResult: { players: 'rec:RoPlayers', character: 'ref:character|null', flag: 'bool', full: 'bool', counts: 'list:rec:RoCount' },
    RoQueryArgs: { unit: 'ref:unit' },
    RoQueryResult: { alive: 'bool' },
    RoActionArgs: { op: 'enum:RoActionOp', character: 'ref:character|null', unit: 'ref:unit|null', team: 'f64', name: 'str|null', players: 'rec:RoPlayers|null' },
    RoActionResult: { character: 'ref:character|null', name: 'str|null', players: 'rec:RoPlayers|null' },
  },

};
