// Geometry and immutable weapon facts used by fire eligibility checks.
export default {
  enums: { AiWeaponOp: ['coneStart', 'origin', 'forward', 'conePoint', 'coneFacts', 'aimer', 'range', 'rangePoint',
    'protectionStart', 'reference', 'sphere'] },
  records: {
    AiWeaponArgs: { op: 'enum:AiWeaponOp', index: 'u32' },
    AiWeaponResult: { flag: 'bool', values: 'list:f64', point: 'f64x3' },
  },
  imports: { aiWeapon: { args: 'AiWeaponArgs', result: 'AiWeaponResult' } },
};
