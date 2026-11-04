// Live log-trap commands. World effects carry the current state so hull
// deaths and presentation callbacks may reenter the same trap safely.
export default {
  enums: {
    TrPhase: ['down', 'reset', 'armed', 'swing'],
    TrOperation: ['setHealth', 'begin', 'step', 'crush', 'catches', 'stateRecord'],
    TrQueryOp: ['alive', 'sphere', 'side'],
    TrActionOp: ['clearAttacker', 'setHealth', 'setDying', 'hideRoot', 'show', 'sound', 'effect'],
  },
  records: {
    TrState: { built: 'bool', health: 'f64', phase: 'enum:TrPhase', started: 'f64|null', time: 'f64', sprung: 'bool', tick: 'f64', crushed: 'f64' },
    TrSettings: { maxHealth: 'f64', reach: 'f64', minRadius: 'f64', deathTime: 'f64', buildFrames: 'f64|null', triggerFrames: 'f64|null', triggerTeam: 'str|null', sound: 'str|null', deathEffect: 'str|null', point: 'f64x3', effectAt: 'f64x3' },
    TrInput: { op: 'enum:TrOperation', index: 'f64', state: 'rec:TrState', settings: 'rec:TrSettings', dt: 'f64', tick: 'f64', value: 'f64', phase: 'enum:TrPhase', started: 'f64|null', vehicle: 'ref:vehicle|null', vehicles: 'list:(ref:vehicle|null)' },
    TrResult: { state: 'rec:TrState', flag: 'bool', record: 'f64x3' },
    TrQueryArgs: { op: 'enum:TrQueryOp', index: 'f64', vehicle: 'ref:vehicle|null' },
    TrQueryResult: { flag: 'bool', number: 'f64', position: 'f64x3', name: 'str|null' },
    TrActionArgs: { op: 'enum:TrActionOp', index: 'f64', state: 'rec:TrState', vehicle: 'ref:vehicle|null', number: 'f64', flag: 'bool', name: 'str|null', position: 'f64x3' },
    TrActionResult: { state: 'rec:TrState' },
  },
  imports: {
    trapsQuery: { args: 'TrQueryArgs', result: 'TrQueryResult' },
    trapsAction: { args: 'TrActionArgs', result: 'TrActionResult' },
  },
};
