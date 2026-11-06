// Game.step's phase order. Each service reads the current facade at its
// source slot; no state table is copied into a callback scheduler. Bodies,
// fleet, live objects and the battle tail keep their own native boundaries.
export function stepWorld(host, call, tick) {
  call('start');
  if (host.water) call('water');
  call('clockAxes', tick / 60);
  for (let index=0;index<4;index++) {
    const taken=call('takeSquad',index);
    if (taken && !host.dead && host.playerCommand) call('squad',index);
  }
  // Counting the dormant viewpoint remains before AI and lazy shape reads.
  call('playerCount');call('eye');
  const eyes=host.eyeView && !host.dead;
  if (host.deathCamera) call('deathCamera');
  else if (host.remote) call('remoteCamera');
  else if (host.driver) call('driverCamera');
  else if (host.gunner) call('gunnerCamera');
  else {
    call('zoom');
    if (!(host.server && host.removed)) call('camera',0,eyes);
  }
  call('cameraMatrix');
  if (host.ai) call('ai');
  call('nativeUnits');
  if (host.live) call('live');
  call('ordnance');call('mineTargets');call('items');
  call('battlefield');call('battle');call('target');
  if (host.grenadeUpdate) {
    call('grenades');
    while (call('nextGrenade')) call('grenade');
  }
  call('spawnFlush');
}
