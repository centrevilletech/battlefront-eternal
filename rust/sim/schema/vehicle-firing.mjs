// Recorded geometry state for native archive replays. Live mounted geometry
// belongs to the required F2 phase and has no per-method host adapter.
export default {
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
  },
};
