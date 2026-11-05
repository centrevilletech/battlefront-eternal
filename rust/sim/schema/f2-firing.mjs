// Persistent mounted weapons and their phase publications. Lifecycle records
// carry immutable rig keys once; ordinary ticks carry frozen foreign inputs.
export default {
  records: {
    FfPhase: { weapons: 'u32', cables: 'u32', events: 'u32' },
    FfSupplyResult: { changed: 'bool', output: 'u32' },
    FfSupply: { hull: 'ref:vehicle', life: 'f64', amount: 'f64' },
    FfInputs: { $layout: true, count: 'u32' },
    FfWeaponChange: { $layout: true, hull: 'ref:vehicle', seat: 'u32', weapon: 'u32', input: 'rec:FfWeaponInput' },
    FfWeaponInput: { $layout: true, channel: 'f64', target: 'i32', hasRandom: 'bool', floor: 'f64', spreadScale: 'f64', sniperScale: 'f64', yawSpreadOverride: 'f64|null' },
    FfMuzzle: { emitter: 'union:BeamSource', node: 'f64', aimer: 'f64' },
    FfWeaponBirth: { state: 'rec:Weapon', ordnance: 'def:ordnance|null', tow: 'bool', range: 'f64', cableLength: 'f64', channel: 'f64', relationMask: 'u32', categoryMask: 'u32',
      weaponNode: 'f64', fallbackNode: 'f64', directionNodes: 'list:f64', outside: 'list:f64',
      emitter: 'union:BeamSource', emitterNode: 'f64', emitterAimer: 'f64', muzzles: 'list:rec:FfMuzzle' },
    FfSeatBirth: { weapons: 'list:rec:FfWeaponBirth' },
    FfLeg: { name: 'str', thigh: 'vec3|null', calf: 'vec3|null', foot: 'vec3|null' },
    FfBirth: { hull: 'ref:vehicle', life: 'f64', commandWalker: 'bool', legTop: 'f64', legs: 'list:rec:FfLeg', seats: 'list:rec:FfSeatBirth' },
    FfWeaponOut: { state: 'rec:Weapon', barrel: 'f64', kicks: 'list:(f64|undefined)' },
    FfSeatOut: { consumed: 'bool', weapons: 'list:rec:FfWeaponOut' },
    FfHullOut: { hull: 'ref:vehicle', life: 'f64', seats: 'list:rec:FfSeatOut' },
    FfCableOut: { source: 'ref:vehicle', target: 'ref:vehicle', seat: 'u32', weapon: 'u32', length: 'f64', start: 'vec3', end: 'vec3' },
    FfDraw: { hull: 'ref:vehicle', seat: 'u32', weapon: 'u32' },
    // Unit hit shapes and other pre-F3 registry facts remain foreign. This
    // query is synchronous so lazy shape timing is retained.
    FfTargetQuery: { index: 'u32', viewer: 'ref:vehicle', shapes: 'bool', aim: 'bool' },
    FfTargetFacts: { alive: 'bool', eligible: 'bool', relation: 'f64', category: 'u32', center: 'vec3', radius: 'f64', aim: 'vec3', body: 'f64' },
    FfShot: { hull: 'ref:vehicle', life: 'f64', seat: 'u32', weapon: 'u32', state: 'rec:Weapon', owner: 'union:OrdnanceOwner',
      yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64', origin: 'vec3', direction: 'vec3' },
    FfFired: { shot: 'rec:FfShot', events: 'u32' },
    FfTow: { source: 'ref:vehicle', life: 'f64', seat: 'u32', weapon: 'u32', reason: 'str' },
  },
  imports: {
    f2Posed: { args: 'VehNothing', result: null },
    f2WeaponDraw: { args: 'FfDraw', result: 'VehNumber' },
    f2TargetFacts: { args: 'FfTargetQuery', result: 'FfTargetFacts' },
    f2Fired: { args: 'FfFired', result: null },
  },
  events: { f2Shot: 'FfShot', f2Tow: 'FfTow' },
};
