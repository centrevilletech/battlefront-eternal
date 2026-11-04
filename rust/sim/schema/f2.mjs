// Persistent vehicle-phase boundaries. Addresses point at generated row blocks;
// no lifecycle or command needs to export the complete fleet to name one row.
export default {
  enums: {
    F2SeatOp: ['board', 'requestExit', 'unboard', 'vacate', 'move', 'displace', 'promote', 'cycle', 'look', 'network', 'destroyed', 'release', 'man', 'turretLeave', 'turretDestroyed', 'turretRequestExit'],
    F2SeatActionKind: ['controller', 'control', 'policy', 'boarded', 'unboarded', 'moved', 'kill', 'body', 'raycast', 'flush'],
    F2TurretEventKind: ['bind', 'sound', 'fired'],
  },
  records: {
    // Flat rows let the phase decode one input at a time without temporary lists.
    F2SeatHeader: { weapons: 'u32', weaponCount: 'u32', units: 'u32', unitCount: 'u32', controls: 'u32', controlCount: 'u32', looks: 'u32', lookCount: 'u32', mouseScale: 'f64', invert: 'bool', flightScale: 'f64', flightInvert: 'bool' },
    F2SeatSample: { dx: 'f64', dy: 'f64' },
    F2SeatUnitIn: { unit: 'ref:unit', generation: 'f64', alive: 'bool', dead: 'bool', human: 'bool', networkHuman: 'bool|undefined', walker: 'bool', team: 'f64', removed: 'bool', hasActor: 'bool', tickCount: 'f64', soldier: 'rec:Soldier', intent: 'rec:Intent', noEnter: 'f64', zoomStep: 'f64', zoom: 'f64', firstPerson: 'bool', blowApart: 'bool', combatDead: 'bool', targetable: 'bool|undefined', mounted: 'bool|undefined', gunUnderwater: 'bool', aimOverride: 'vec3|null|undefined' },
    F2SeatBody: { position: 'vec3', previous: 'vec3', settled: 'vec3', velocity: 'vec3', normal: 'vec3', aimYaw: 'f64', aimPitch: 'f64', air: 'enum:Air|null', posture: 'enum:Posture', grounded: 'bool', dive: 'f64', diveFrom: 'enum:Posture', lock: 'f64', lookX: 'f64', lookY: 'f64', fuel: 'f64', stateTime: 'f64', passThrough: 'ref:body|null' },
    F2SeatEdges: { reload: 'bool', use: 'bool', exit: 'bool', jump: 'bool', crouch: 'bool', prone: 'bool', nextPrimary: 'bool', previousPrimary: 'bool', nextSecondary: 'bool', zoom: 'bool' },
    F2SeatUnitOut: { unit: 'ref:unit', seat: 'ref:seatLease|null', targetable: 'bool|undefined', tickCount: 'f64', soldier: 'rec:F2SeatBody', intent: 'rec:F2SeatEdges', mounted: 'bool|undefined', gunUnderwater: 'bool', aimOverride: 'vec3|null|undefined', zoomStep: 'f64' },
    F2SeatEffect: { kind: 'enum:SeatEffectKind', unit: 'ref:unit', point: 'vec3', yaw: 'f64' },
    F2Latched: { control: 'ref:seatUnit' },
    F2SeatFire: { lease: 'ref:seatLease', pressed: 'f64x2', reload: 'bool' },
    F2HullNode: { hull: 'ref:vehicle', node: 'u32', matrix: 'mat4', world: 'mat4' },
    F2SeatIo: { emitterMask: 'u32', ordnance: 'u32', nodes: 'u32', words: 'u32', input: 'u32', capacity: 'u32', headerWords: 'u32', unitWords: 'u32', controlWords: 'u32', lookWords: 'u32', weaponWords: 'u32', leases: 'u32', units: 'u32', controls: 'u32', effects: 'u32', fire: 'u32', turrets: 'u32', events: 'u32', ticks: 'f64', turretTicks: 'f64', turretTime: 'f64' },
    F2TurretBirth: { settings: 'rec:PtSettings', turret: 'rec:PcTurret', weapon: 'rec:Weapon|null', ordnance: 'def:ordnance|null', channel: 'f64', collide: 'bool' },
    F2TurretLife: { turret: 'ref:turret', alive: 'bool', attacker: 'union:Attacker', center: 'vec3', radius: 'f64' },
    F2TurretHeader: { turret: 'ref:turret' },
    F2TurretInput: { index: 'u32', hasRandom: 'bool', floor: 'f64', spreadScale: 'f64', sniperScale: 'f64', yawSpreadOverride: 'f64|null' },
    F2TurretOut: { header: 'rec:F2TurretHeader', state: 'rec:PtState', lease: 'ref:seatLease|null', gunner: 'ref:unit|null', weapon: 'rec:Weapon|null' },
    F2TurretDraw: { turret: 'u32' },
    F2TurretLock: { turret: 'u32', origin: 'vec3', direction: 'vec3', lease: 'ref:seatLease' },
    F2TurretLockAnswer: { id: 'f64', generation: 'f64', alive: 'bool', found: 'bool' },
    F2TurretEvent: { kind: 'enum:F2TurretEventKind', turret: 'ref:turret', round: 'ref:round|null', bolt: 'ref:bolt|null', barrel: 'f64', owner: 'ref:unit|null', origin: 'vec3', direction: 'vec3', yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64' },
    F2SeatCommand: { op: 'enum:F2SeatOp', unit: 'ref:unit|null', unitLife: 'f64', hull: 'ref:vehicle|null', hullLife: 'f64', lease: 'ref:seatLease|null', index: 'f64', placement: 'bool', reason: 'enum:SeatReason', safe: 'bool', force: 'bool', promote: 'bool|undefined', post: 'ref:post|null', direction: 'f64' },
    F2SeatResult: { accepted: 'bool', value: 'f64', lease: 'ref:seatLease|null' },
    F2SeatAction: { kind: 'enum:F2SeatActionKind', unit: 'ref:unit|null', holder: 'f64', turret: 'bool', index: 'f64', lease: 'ref:seatLease|null', post: 'ref:post|null', reason: 'enum:SeatReason', effect: 'enum:SeatEffectKind', point: 'vec3', direction: 'vec3', yaw: 'f64', flag: 'bool', second: 'bool', attacker: 'union:Attacker' },
    F2SeatAnswer: { control: 'ref:seatUnit|null', value: 'f64|undefined', flag: 'bool', point: 'vec3' },

    F2HullLife: {
      vehicle: 'ref:vehicle', generation: 'f64', visible: 'bool',
      flightClass: 'enum:FlightClass|null', minSpeed: 'f64|undefined',
      midSpeed: 'f64|undefined', boostSpeed: 'f64|undefined', speed: 'f64', turnRate: 'f64',
    },
    F2Pad: { index: 'f64', remaining: 'f64', vehicle: 'ref:vehicle|null' },
  },
  commands: {
    // Detached scalar dispatch releases the instance before a reentrant hook.
    f2Seat: { args: 'F2SeatCommand', result: 'F2SeatResult', rust: 'vehicles::f2_seats::command', detached: true },
  },
  imports: { f2Foreign: { args: 'VehNumber', result: null }, f2SeatAction: { args: 'F2SeatAction', result: 'F2SeatAnswer' }, f2TurretDraw: { args: 'F2TurretDraw', result: 'VehNumber' }, f2TurretLock: { args: 'F2TurretLock', result: 'F2TurretLockAnswer' } },
};
