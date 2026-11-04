// Drive real mounted bodies through takeover, firing, both exit paths,
// and a turret death. The actions run inside the step so capture sees
// their method frames. Load this probe in plain and captured runs alike.
import assert from 'node:assert/strict';
let game, server, turret, gunner, shots = 0;
const actions = [];
export function battle(context) {
  ({ game, server } = context);
  const crews = game.turretSeats, fired = crews.onFired;
  crews.onFired = (...args) => { if (args[0] === turret) shots++; return fired?.(...args); };
  const world = context.world, step = world.step;
  world.step = function (...args) { act(game.worldClock.tick + 1); return step.apply(this, args); };
}
function act(tick) {
  const crews = game.turretSeats, human = server.actor.unit;
  if (tick === 10) {
    turret = crews.turrets.find(t => t.weapon); assert.ok(turret, 'a real armed turret is available'); turret.setHealth(turret.maxHealth);
  }
  if (tick === 40 || tick === 230) {
    if (turret.lease) crews.vacate(turret.lease, { placement: false });
    gunner = game.units.all().find(u => u.soldier && !u.networkHuman && crews.canMan(u, turret));
    // The first normal wave can be fifteen seconds away. Use the real
    // spawn manager for one prepared bot so this short probe reaches its
    // crew path with normal spawn and population accounting.
    if (!gunner) {
      const spawning = game.ai.spawning;
      const character = spawning.teams[human.team].characters.find(c => !c.human && !c.unit && c.spawn.post);
      assert.ok(character, 'a prepared bot is available'); gunner = spawning.spawn(character);
    }
    assert.ok(gunner && crews.man(gunner, turret), `a real bot mans the turret at ${tick}`); actions.push('bot');
  }
  if (tick === 45 || tick === 190) {
    assert.ok(human.alive && crews.man(human, turret), 'the network human takes the turret'); actions.push('human');
  }
  if (tick === 180) { assert.ok(crews.requestExit(human, { placement: false })); actions.push('request'); }
  if (tick === 220) { assert.ok(crews.leave(human, { placement: true })); actions.push('leave'); }
  if (tick === 260) {
    turret.damage(turret.maxHealth + 1, human);
    assert.equal(gunner.alive, false, 'destroying the turret kills its real occupant'); actions.push('destroyed');
  }
}
export function ended() {
  assert.deepEqual(actions, ['bot', 'human', 'request', 'human', 'leave', 'bot', 'destroyed']);
  assert.ok(shots > 0, 'the manned turret fired');
  console.error(JSON.stringify({ turretCrewsProbe: { actions, shots } }));
}
