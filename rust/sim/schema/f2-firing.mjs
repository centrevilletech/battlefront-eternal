// Persistent mounted weapons and their phase publications. Lifecycle records
// carry immutable rig keys once; ordinary ticks carry frozen foreign inputs.
export default {
  records: {
    FfMuzzle: { emitter: 'union:BeamSource' },
    FfWeaponBirth: { state: 'rec:Weapon', ordnance: 'def:ordnance|null', tow: 'bool', range: 'f64', cableLength: 'f64', channel: 'f64',
      effect: 'str|null', weaponNode: 'f64', fallbackNode: 'f64', directionNodes: 'list:f64', outside: 'list:f64',
      emitter: 'union:BeamSource', muzzles: 'list:rec:FfMuzzle' },
    FfSeatBirth: { weapons: 'list:rec:FfWeaponBirth' },
    FfLeg: { name: 'str', thigh: 'vec3|null', calf: 'vec3|null', foot: 'vec3|null' },
    FfBirth: { hull: 'ref:vehicle', life: 'f64', legTop: 'f64', legs: 'list:rec:FfLeg', seats: 'list:rec:FfSeatBirth' },
    FfWeaponOut: { state: 'rec:Weapon', barrel: 'f64', kicks: 'list:(f64|undefined)' },
    FfSeatOut: { consumed: 'bool', weapons: 'list:rec:FfWeaponOut' },
    FfHullOut: { hull: 'ref:vehicle', life: 'f64', seats: 'list:rec:FfSeatOut' },
    FfCableOut: { source: 'ref:vehicle', target: 'ref:vehicle', seat: 'u32', weapon: 'u32', length: 'f64', start: 'vec3', end: 'vec3' },
    FfOutput: { hulls: 'list:rec:FfHullOut', cables: 'list:rec:FfCableOut' },
    FfDraw: { hull: 'ref:vehicle', seat: 'u32', weapon: 'u32' },
    // Unit hit shapes and other pre-F3 registry facts remain foreign. This
    // query is synchronous so lazy shape timing is retained.
    FfTargetQuery: { index: 'u32', viewer: 'ref:vehicle' },
    FfTargetFacts: { eligible: 'bool', relation: 'f64', category: 'u32', center: 'vec3', radius: 'f64', aim: 'vec3', body: 'f64' },
    FfShot: { hull: 'ref:vehicle', life: 'f64', seat: 'u32', weapon: 'u32', owner: 'union:OrdnanceOwner',
      yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64', origin: 'vec3', direction: 'vec3' },
    FfTow: { source: 'ref:vehicle', life: 'f64', seat: 'u32', weapon: 'u32', reason: 'str' },
  },
  imports: {
    f2WeaponDraw: { args: 'FfDraw', result: 'VehNumber' },
    f2TargetFacts: { args: 'FfTargetQuery', result: 'FfTargetFacts' },
    f2Fired: { args: 'FfShot', result: null },
  },
  events: { f2Shot: 'FfShot', f2Tow: 'FfTow' },
};
