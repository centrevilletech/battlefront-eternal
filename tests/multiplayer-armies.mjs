// Requires the operator's imported Theed gameplay content.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
const root = resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch = async name => {
  const file = resolve(root, `.${name}`);
  assert.ok(file.startsWith(root + sep));
  try { return new Response(await readFile(file)); }
  catch (error) { if (error.code === 'ENOENT') return new Response('', {status:404}); throw error; }
};
const game = await createGame({headless:true, query:'?map=nab2&side=rep&ai=classic&seed=1'});
const world = createMultiplayerWorld(game, {maxPlayers:66});
assert.deepEqual(world.roster.counts(), [{team:1,humans:0,total:32},{team:2,humans:0,total:32}]);
const slots = game.battle.characters.length;
const tickets = {...game.battle.reinforcements};
for (let i=0;i<64;i++) await world.addPlayer(`player-${i}`,{team:i%2+1});
assert.equal(game.battle.characters.length, slots, 'connections reuse stable army slots');
assert.deepEqual(world.roster.counts(), [{team:1,humans:32,total:32},{team:2,humans:32,total:32}]);
assert.deepEqual(game.battle.reinforcements, tickets, 'replacing bots does not spend tickets');
assert.equal(new Set([...world.players.values()].map(p=>p.unit.id)).size,64);
await assert.rejects(world.addPlayer('overflow',{team:1}),/team full/);
await world.addPlayer('spectator',{spectator:true});
assert.equal(game.battle.characters.length, slots, 'spectators do not consume army slots');
assert.equal(world.roster.switchTeam(world.players.get('player-0').character,2),false,'cannot overfill target team');
for (let i=0;i<60;i++) world.step();
for (const team of [1,2]) {
  assert.ok(game.ai.spawning.teams[team].living<=32);
  assert.equal(game.ai.spawning.teams[team].characters.filter(c=>c.unit?.alive).length,game.ai.spawning.teams[team].living);
}
const departing = world.players.get('player-0');
world.removePlayer('player-0');
assert.equal(world.roster.counts()[0].humans,32,'reconnect grace retains the slot');
departing.disconnectedAt = game.worldClock.tick - 3600;
world.step();
assert.equal(world.players.has('player-0'),false);
assert.equal(world.roster.counts()[0].humans,31,'expired connection becomes a bot');
assert.equal(world.roster.counts()[0].total,32);
await world.addPlayer('replacement',{team:1});
assert.equal(game.battle.characters.length, slots);
assert.equal(world.roster.counts()[0].humans,32);
console.log(JSON.stringify({status:'passed',armies:world.roster.counts(),connections:world.players.size,ticks:game.worldClock.tick}));
