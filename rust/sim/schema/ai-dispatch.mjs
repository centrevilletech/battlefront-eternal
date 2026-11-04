// Engagement observations and writes shared by combat and vehicle policy.
export default {
  enums: { AiDispatchOp: ['init', 'kinds', 'roleAI', 'rolePlain', 'roleName', 'ownerHero', 'targetHero',
    'select', 'aux', 'action', 'inBand', 'pattern', 'probability', 'roll', 'vehicleHook', 'vehicleDispatch', 'rangeStart', 'difficulty',
    'roleStart', 'skill', 'numericAction'] },
  records: {
    AiDispatchArgs: { op: 'enum:AiDispatchOp', number: 'f64' },
    AiDispatchResult: { flag: 'bool', number: 'f64', values: 'list:f64', text: 'str' },
  },
  imports: { aiDispatch: { args: 'AiDispatchArgs', result: 'AiDispatchResult' } },
};
