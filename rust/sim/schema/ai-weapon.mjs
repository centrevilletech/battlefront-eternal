// Geometry and immutable weapon facts used by fire eligibility checks.
export default {
  enums: { AiWeaponOp: ['coneStart', 'origin', 'forward', 'conePoint', 'coneFacts', 'aimer', 'range', 'rangePoint',
    'protectionStart', 'reference', 'sphere', 'validationStart', 'target', 'lead', 'targetPoint',
    'spreadStart', 'sniper', 'spreadValues', 'spreadWrite',
    'choiceStart', 'choiceOrigin', 'choiceAim', 'choiceWeapon', 'choiceUtility', 'choiceCapacity', 'choiceRange',
    'choiceSpeed', 'choiceGrenade', 'choiceCluster', 'choiceRoll', 'choiceWrite', 'choiceSelected', 'choiceSwitch'] },
  records: {
    AiWeaponArgs: { op: 'enum:AiWeaponOp', index: 'u32', number: 'f64' },
    AiWeaponResult: { flag: 'bool', values: 'list:f64', point: 'f64x3' },
  },
  imports: { aiWeapon: { args: 'AiWeaponArgs', result: 'AiWeaponResult' } },
};
