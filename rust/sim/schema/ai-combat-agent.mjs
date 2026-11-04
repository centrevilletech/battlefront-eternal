// Combat-agent initialization and acceptance policy service observations.
export default {
  enums: { AiCombatAgentOp: ['goalMode', 'request', 'goalPoint', 'goalTarget', 'cleanup', 'base',
    'mode', 'state', 'patternExit', 'unit', 'speed', 'mount', 'known', 'cone', 'sourcePoint',
    'target', 'distanceStart', 'distanceTarget', 'engage', 'mountReach', 'mountCall', 'boarding',
    'shield', 'shieldMax', 'shieldValue', 'switchShield', 'fireA', 'fireB'] },
  records: {
    AiCombatAgentArgs: { op: 'enum:AiCombatAgentOp', id: 'u32', number: 'f64' },
    AiCombatAgentResult: { flag: 'bool', number: 'f64', values: 'list:f64' },
  },
  imports: { aiCombatAgent: { args: 'AiCombatAgentArgs', result: 'AiCombatAgentResult' } },
};
