// Attack-pattern lifecycle and policy service observations.
export default {
  enums: { AiPatternOp: ['field', 'initValue', 'request', 'flush', 'priorCapture', 'random', 'time',
    'int', 'add', 'agent', 'state', 'stateValue', 'stop', 'roll', 'action', 'coverInit',
    'jetEnter', 'delay', 'gotoTarget', 'vehicleEnter', 'stance', 'coverExit', 'jetExit',
    'vehicleExit', 'vehicleTick', 'cleanup', 'range', 'rangeCompare', 'findCover', 'reserve', 'priorFrom',
    'jetUnit', 'jetJump', 'jetBorder', 'jetFuel', 'jetReady', 'jetHints', 'hintFind', 'sampleTime', 'transition', 'standingUnit', 'speed', 'sidestep',
    'stationary', 'face', 'helper', 'arrived', 'stopHelper', 'reposition', 'redirect', 'flank',
    'coverUpdate', 'coverDone', 'restore', 'fieldTruthy', 'plus', 'flying', 'hintTick', 'freshTarget',
    'agentArrived', 'look', 'meleeStart', 'directClear', 'canFollow', 'follow', 'gotoMelee', 'pointRead', 'pointWrite', 'uniform',
    'repositionWeapon', 'repositionRange', 'repositionBands', 'rangeDraw', 'targetPoint', 'geometryStart',
    'movePoint', 'captureMove', 'lowerBand', 'upperBand', 'geometryStop', 'dodge',
    'wasFacing', 'facingTarget', 'stepToBand', 'stepped', 'bandStart', 'redirectAction', 'sideClone',
    'forward', 'lateralRange', 'geometryRange', 'angleValue', 'directStart', 'directPosition',
    'pointClear', 'los', 'jetObstructed', 'jetMove', 'jetRequest', 'directTime'] },
  records: {
    AiPatternArgs: { op: 'enum:AiPatternOp', id: 'u32', number: 'f64', other: 'f64', point: 'f64x3' },
    AiPatternResult: { flag: 'bool', number: 'f64', other: 'f64', point: 'f64x3', values: 'list:f64' },
  },
  imports: { aiPattern: { args: 'AiPatternArgs', result: 'AiPatternResult' } },
};
