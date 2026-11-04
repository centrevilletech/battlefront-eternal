// Weapon facts, validation queries and shared draws for trigger shaping.
export default {
  enums: { AiFireOp: ['start', 'selected', 'requests', 'target', 'validate', 'charge', 'single', 'cannon',
    'gravity', 'limit', 'chargeDraw', 'chargeHeld', 'drawRange', 'drawInt', 'sniper', 'idle', 'deployer',
    'clearRequests', 'burst', 'burstHeat', 'protected', 'spread', 'buttons'] },
  records: {
    AiFireArgs: { op: 'enum:AiFireOp', number: 'f64', other: 'f64' },
    AiFireResult: { number: 'f64', flag: 'bool', values: 'list:f64' },
  },
  imports: { aiFire: { args: 'AiFireArgs', result: 'AiFireResult' } },
};
