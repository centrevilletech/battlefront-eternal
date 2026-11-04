// Live object access is a host boundary; the native driver owns lifecycle order.
export default {
  enums: {
    NetworkLifecycleCall: ['battleOver', 'roster', 'current', 'alive', 'deadTime', 'post', 'prepared',
      'previous', 'reuse', 'acquire', 'retireFlags', 'retireHandler', 'retire', 'unitFlags', 'combatPlayer',
      'team', 'character', 'characterUnit', 'characterClass', 'characterReady', 'characterTimer',
      'previousAlive', 'teamExists', 'livingAdd', 'remoteClear', 'remoteBind', 'remoteStep', 'publish',
      'generations', 'generation', 'spawn', 'weapons', 'nextWeapon', 'weapon', 'spawnStatus', 'emit',
      'unspawnUnit', 'unspawnFlags', 'invisible', 'unspawnTime', 'livingRemove', 'unspawnStatus',
      'player', 'disconnect', 'stop', 'remoteExit', 'removeUnit', 'vehicle', 'turret', 'unboard', 'leave'],
  },
  records: {
    NetworkLifecycleArgs: { op: 'enum:NetworkLifecycleCall', value: 'f64' },
    NetworkLifecycleReply: { flag: 'bool', other: 'bool', value: 'f64', second: 'f64' },
  },
  imports: { networkLifecycle: { args: 'NetworkLifecycleArgs', result: 'NetworkLifecycleReply' } },
};
