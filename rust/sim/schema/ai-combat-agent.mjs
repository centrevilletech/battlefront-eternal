// Combat-agent initialization and acceptance policy service observations.
export default {
  enums: { AiCombatAgentOp: ['goalMode', 'request', 'goalPoint', 'goalTarget', 'cleanup', 'base',
    'mode', 'state', 'patternExit', 'unit', 'speed', 'mount', 'known', 'cone', 'sourcePoint',
    'target', 'distanceStart', 'distanceTarget', 'engage', 'mountReach', 'mountCall', 'boarding',
    'shield', 'shieldMax', 'shieldValue', 'switchShield', 'fireA', 'fireB',
    'entryStart', 'capturedKind', 'capturedMode', 'stop', 'investigateStart', 'investigateNormal', 'investigateValues', 'investigatePoint',
    'look', 'stance', 'nextLook', 'captureTarget', 'dispatch', 'storePattern', 'hasPattern', 'initPattern',
    'evade', 'alongPath', 'behaveStart', 'capturedTime', 'select', 'assignWeapon', 'alert',
    'jetState', 'flying', 'interrupt', 'aligned', 'field', 'fieldTruthy', 'fastTurn', 'patternUpdate',
    'sensorStart', 'sensorDistance', 'samePattern', 'skipFire', 'raiseShield', 'helperTick', 'fireControls',
    'freshTarget', 'geometryUnit', 'targetAim', 'origin', 'forward', 'pointRead', 'pointWrite', 'inputNumber',
    'random', 'range', 'int', 'capturePoint', 'unitPosition', 'setPoint', 'lookCurrent', 'incrementLooks',
    'evadeStart', 'evadeValues', 'dodge', 'move', 'inputAdd', 'coneStart', 'boardStart', 'boardQuery', 'boardLength', 'boardValid'] },
  records: {
    AiCombatAgentArgs: { op: 'enum:AiCombatAgentOp', id: 'u32', number: 'f64', point: 'f64x3' },
    AiCombatAgentResult: { flag: 'bool', number: 'f64', values: 'list:f64', point: 'f64x3' },
  },
  imports: { aiCombatAgent: { args: 'AiCombatAgentArgs', result: 'AiCombatAgentResult' } },
};
