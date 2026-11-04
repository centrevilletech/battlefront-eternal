// Commands for ordinary authoritative Weapon objects. Their identity,
// immutable definitions and foreign generators remain with the holder.
export default {
  enums: {
    WpOperation: ['update', 'deselect', 'clear', 'lock', 'reload', 'base', 'spread', 'target'],
    WpCallback: ['random', 'legacy', 'admit', 'pick', 'start', 'seeded'],
    WpShotKind: ['round', 'repair', 'meleeStart', 'meleeHit', 'meleeEnd'],
  },
  records: {
    // `weapon.level` is the definition cursor; the ordinary object's
    // numeric field can intentionally differ until its next assignment.
    WpState: { weapon: 'rec:Weapon', numericLevel: 'f64', numericSeed: 'f64|null' },
    WpTarget: { id: 'f64', generation: 'f64', alive: 'bool' },
    WpInput: {
      op: 'enum:WpOperation', state: 'rec:WpState', dt: 'f64', selected: 'bool',
      held: 'bool', pressed: 'bool', seed: 'f64|null', late: 'f64', reload: 'bool',
      stick: 'f64', posture: 'enum:Posture|undefined', zoomed: 'bool', repairTarget: 'bool', target: 'rec:WpTarget|null',
    },
    WpShot: { kind: 'enum:WpShotKind', yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64', admission: 'f64|null', swing: 'f64' },
    WpOutput: { state: 'rec:WpState', shots: 'list:rec:WpShot', flag: 'bool', spread: 'f64x2' },
    WpAction: { kind: 'enum:WpCallback', state: 'rec:WpState', first: 'bool' },
    WpReply: { state: 'rec:WpState', value: 'f64', admission: 'u32', target: 'rec:WpTarget|null' },
  },
  imports: { weaponAction: { args: 'WpAction', result: 'WpReply' } },
};
