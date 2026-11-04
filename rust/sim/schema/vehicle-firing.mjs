// Geometry commands use a hull's retained prepared rig. Object identity
// and the clock stay with its host; all transform arithmetic stays here.
export default {
  enums: { VgOperation: ['muzzle', 'muzzleSeat', 'limits', 'reticle', 'bot', 'nose', 'direction', 'beam', 'spread', 'tow'] },
  records: {
    VgNode: { position: 'vec3', quaternion: 'quat', scale: 'vec3', matrix: 'mat4', matrixWorld: 'mat4',
      auto: 'bool', worldAuto: 'bool', needs: 'bool' },
    VgAimer: { bone: 'f64', directionNode: 'f64', facing: 'vec3', posed: 'bool', onTarget: 'bool',
      inLimits: 'bool|undefined', outside: 'f64' },
    VgMuzzle: { node: 'f64', aimer: 'f64', barrel: 'f64', kickAt: 'f64|undefined' },
    VgWeapon: { barrel: 'f64', weaponNode: 'f64', fallbackNode: 'f64', aimers: 'list:rec:VgAimer', muzzles: 'list:rec:VgMuzzle' },
    VgSeat: { yaw: 'f64', pitch: 'f64', target: 'vec3|null', leased: 'bool', human: 'bool', aiming: 'bool', aim: 'vec3',
      turret: 'f64', weapons: 'list:rec:VgWeapon' },
    VgState: { flying: 'bool', center: 'vec3', size: 'vec3', matrix: 'mat4', quaternion: 'quat', seatIndex: 'u32',
      seats: 'list:rec:VgSeat', nodes: 'list:rec:VgNode' },
    VgInput: { op: 'enum:VgOperation', hull: 'ref:vehicle', state: 'rec:VgState', seat: 'u32', weapon: 'u32', aimer: 'f64',
      from: 'vec3', target: 'vec3|null', fallback: 'vec3|null', out: 'vec3', normal: 'vec3', beamFrom: 'vec3',
      node: 'f64', yaw: 'f64', pitch: 'f64', tow: 'bool', towTarget: 'f64' },
    VgOutput: { state: 'rec:VgState', out: 'vec3', normal: 'vec3', beamFrom: 'vec3', node: 'f64', aimer: 'f64', emitter: 'f64', flag: 'bool' },
    VgClock: { state: 'rec:VgState' },
    VgTime: { state: 'rec:VgState', value: 'f64' },
  },
  imports: { vehicleGeometryClock: { args: 'VgClock', result: 'VgTime' } },
};
