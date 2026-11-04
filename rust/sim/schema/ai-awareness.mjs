// Unit observations and controller notifications used by acoustic awareness.
export default {
  enums: { AiAwarenessOp: ['alive', 'motion', 'weapon', 'active', 'mute', 'query', 'recipient', 'notify',
    'livingStart', 'livingNext', 'clear', 'protectQuery', 'protectRecipient', 'protectAppend',
    'dangerRow', 'dangerDraw', 'dangerTime', 'dangerFirst', 'dangerLast', 'dangerQuery', 'dangerRecipient',
    'dangerSkill', 'dangerSkillCompare', 'dangerAdd',
    'dangerWarn', 'dangerHasNotify', 'dangerNotify', 'dangerWrite',
    'shotStart', 'shotSource', 'shotRelation', 'shotHuman', 'damageStart', 'damageRelation', 'damagePacket',
    'alertSeat', 'alertSeatController', 'alertOwn', 'alertCanNotify', 'alertBody', 'alertLeases', 'alertLease',
    'alertCrew', 'alertNotify', 'hazardStart', 'responseUnit', 'responseWeapon', 'hazardAccept',
    'responseKind', 'responseDangerRegion', 'hazardPoint', 'responseResult', 'acousticList', 'acousticStart',
    'acousticNext', 'acousticAccept', 'responseSensor', 'acousticPoint', 'responseHorizontal',
    'responseMountStart', 'responseStay', 'responseReach', 'responseCanReach', 'responseShot',
    'responseSource', 'responsePoint', 'responseAlertAccept', 'responseSight', 'responseDelta',
    'responseArmed', 'responseDispatch', 'responseRandom', 'responseDraw', 'responsePosition', 'responseDodge'] },
  records: {
    AiAwarenessArgs: { op: 'enum:AiAwarenessOp', id: 'u32', number: 'f64', other: 'f64', point: 'f64x3' },
    AiAwarenessResult: { flag: 'bool', number: 'f64', ids: 'list:u32', values: 'list:f64' },
  },
  imports: { aiAwareness: { args: 'AiAwarenessArgs', result: 'AiAwarenessResult' } },
};
