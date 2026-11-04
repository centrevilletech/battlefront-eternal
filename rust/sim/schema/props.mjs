// Live building commands. Constructor geometry is configured once; only
// mutable state and foreign supply targets cross during a command.
export default {
  enums: {
    PbStateName: ['unbuilt', 'built', 'destroyed'],
    PbKind: ['building', 'turret', 'objective'],
    PbMode: ['waiting', 'active'],
    PbOperation: ['setHealth', 'damage', 'regenerate', 'update', 'serve', 'sphere', 'blastSphere', 'built', 'destroyed'],
    PbActionKind: ['show', 'crewDestroyed', 'destroyed', 'explode', 'damaged', 'targetChanged', 'served', 'serveSound'],
  },
  records: {
    PbIdentity: { kind: 'f64', id: 'f64' },
    PbModel: { sphere: 'f64x4|null', low: 'vec3|null', high: 'vec3|null' },
    PbSettings: {
      kind: 'enum:PbKind', maxHealth: 'f64', respawnTime: 'f64', builtAddHealth: 'f64', regenerates: 'bool',
      hasDestroyed: 'bool', hasUnbuilt: 'bool', hasExplosion: 'bool', matrix: 'mat4', models: 'list:(rec:PbModel|null)',
    },
    PbConfiguration: { buildings: 'list:rec:PbSettings' },
    PbState: { state: 'enum:PbStateName', health: 'f64', addHealth: 'f64', lastAttacker: 'rec:PbIdentity', targetGeneration: 'f64|null' },
    PbStation: { building: 'rec:PbState', mode: 'enum:PbMode', active: 'f64', pulse: 'f64', servedPlayer: 'bool', servedHealth: 'bool', servedAmmo: 'bool' },
    PbWeapon: { bottomless: 'bool', ammo: 'f64', clip: 'f64', perShot: 'f64', reserve: 'f64' },
    PbTarget: { id: 'f64', position: 'vec3', vehicle: 'bool', alive: 'bool', player: 'bool', radius: 'f64', health: 'f64', maxHealth: 'f64', weapons: 'list:rec:PbWeapon' },
    PbSupply: { position: 'vec3', radius: 'f64', delay: 'f64', health: 'f64', ammo: 'f64', vehicleHealth: 'f64', vehicleAmmo: 'f64' },
    PbInput: { op: 'enum:PbOperation', index: 'f64', state: 'rec:PbState', crew: 'bool', damaged: 'bool', station: 'rec:PbStation|null', supply: 'rec:PbSupply|null',
      value: 'f64', attacker: 'rec:PbIdentity', hit: 'rec:PbIdentity', targets: 'list:rec:PbTarget', soldiers: 'list:f64', vehicles: 'list:f64' },
    PbResult: { state: 'rec:PbState', station: 'rec:PbStation|null', targets: 'list:rec:PbTarget', number: 'f64', point: 'vec3' },
    PbAction: { kind: 'enum:PbActionKind', index: 'f64', state: 'rec:PbState|null', station: 'rec:PbStation|null', target: 'rec:PbTarget|null',
      point: 'vec3', amount: 'f64', attacker: 'rec:PbIdentity', hit: 'rec:PbIdentity', first: 'f64', gave: 'f64' },
    PbActionResult: { state: 'rec:PbState|null', target: 'rec:PbTarget|null' },
  },
  imports: { pbAction: { args: 'PbAction', result: 'PbActionResult' } },
};
