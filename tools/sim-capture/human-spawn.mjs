// HumanSpawn.update on an offline world. The screen's state and queued
// events are the phase's; bodies, rosters and Classic readiness belong to
// the world. Its callbacks may synchronously call spawned/died/teamChanged.
function state(game, cap) {
  const h = game.humanSpawn, w = h.world, c = w.character();
  return {
    human: cap.export('humanSpawn', h),
    queue: h.queue.map(([name, detail]) => [name, { ...detail }]),
    reads: {
      team: w.team(), dead: w.dead(), over: Boolean(w.over?.()), classic: Boolean(w.classic),
      rosters: Array.from({ length: 8 }, (_, t) => [...w.roster(t)]),
      character: c?.spawn ? [Boolean(c.unit), c.spawn.timer] : null,
    },
  };
}

export const humanSpawn = {
  frames: [{
    method: 'HumanSpawn.update', target: game => game.humanSpawn,
    state: (game, args, cap) => state(game, cap),
    boundaries: game => ['canSpawn', 'setTeam', 'spawn', 'ready', 'kill', 'cancel'].map(name =>
      [`human.${name}`, game.humanSpawn.world, name, (game, cap) => state(game, cap)]),
  }],
};
