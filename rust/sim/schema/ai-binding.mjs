// Combat binding sequencing and event reference observations.
export default {
  enums: { AiBindingOp: ['create','reset','arsenal','weapons','nextWeapon','weaponRandom','hasPerception','clearCandidate',
    'unit','weapon','lead','result','fire','hooked','hasIntent','update','intentWrite','hook','intent',
    'aimStart','forward','difficulty','curveA','ownCurves','aiCurves','aiCurvesCall','difficultyCurves','difficultyCurvesCall','nullCurves',
    'aimer','aimerUpdate','reader','read','dangerState','dangerNew','dangerBind','dangerContact','dangerCompare','dangerWrite','dangerNotify',
    'eventKind','eventSource','eventTarget','eventAttacker','reference','referenceUnit','resolveUnit','resolveAlive','resolveGeneration',
    'resolveResult','referenceHasAlive','referenceAlive','referenceNull','shotController','shotFallback','shotReference','pair','shot',
    'attackerFallback','damage','eligibleAlive','eligibleTargetable','eligibleHealth','eligibleHealthCompare','eligibleRelation','eligibleRelationCompare'] },
  records: {
    AiBindingArgs: { op: 'enum:AiBindingOp', id: 'u32', number: 'f64' },
    AiBindingResult: { flag: 'bool', number: 'f64', values: 'list:f64' },
  },
  imports: { aiBinding: { args: 'AiBindingArgs', result: 'AiBindingResult' } },
};
