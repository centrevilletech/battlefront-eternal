// Attack-pattern lifecycle and policy service observations.
export default {
  enums: { AiPatternOp: ['field', 'initValue', 'request', 'flush', 'priorCapture', 'random', 'time',
    'int', 'add', 'agent', 'state', 'stateValue', 'stop', 'roll', 'action', 'coverInit',
    'jetEnter', 'delay', 'gotoTarget', 'vehicleEnter', 'stance', 'coverExit', 'jetExit',
    'vehicleExit', 'vehicleTick', 'cleanup', 'range', 'rangeCompare', 'findCover', 'reserve', 'priorFrom',
    'jetUnit', 'jetJump', 'jetBorder', 'jetFuel', 'jetReady', 'jetHints', 'hintFind', 'sampleTime', 'transition', 'standingUnit', 'speed', 'sidestep',
    'stationary', 'face', 'helper', 'arrived', 'stopHelper', 'reposition', 'redirect', 'flank',
    'coverUpdate', 'coverDone', 'restore', 'fieldTruthy', 'plus', 'flying', 'hintTick', 'freshTarget',
    'agentArrived', 'look', 'meleeStart', 'directClear', 'canFollow', 'follow', 'gotoMelee'] },
  records: {
    AiPatternArgs: { op: 'enum:AiPatternOp', id: 'u32', number: 'f64', other: 'f64' },
    AiPatternResult: { flag: 'bool', number: 'f64', other: 'f64' },
  },
  imports: { aiPattern: { args: 'AiPatternArgs', result: 'AiPatternResult' } },
};
