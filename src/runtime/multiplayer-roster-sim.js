import { Character } from '../battle.js';
import { putRoInput, RoResultMirror, syncRoResult } from '../sim/gen/index.js';

// The native roster owns slot selection, capacity, fresh scores and team
// swaps. Existing bodies and AI controllers see the state before their
// callbacks, so a removal can reenter the roster safely.
export function createNativeRoster(game, armySize) {
  const classic = Boolean(game.ai?.spawning && game.ai?.classic);
  const owner = classic ? game.spawningSim : game.battleSim;
  const { bridge } = owner, x = bridge.x;
  const input = { words: x.roster_words(0), put: putRoInput };
  const output = { Mirror: RoResultMirror, sync: syncRoResult };
  let players = { capacity: 0, reserved: [] };
  bridge.register({
    rosterQuery({ unit }) { return { alive: Boolean(unit.alive) }; },
    rosterAction({ op, character, unit, team, name, players: current }) {
      owner.sync();
      if (current) players = { capacity: current.capacity, reserved: [...current.reserved] };
      let made = null, named = null;
      try {
        if (op === 'attach') game.ai.classic.attach(character);
        else if (op === 'detach') game.ai.classic.detach(character);
        else if (op === 'botName') named = game.ai.spawning.botName(game.ai.spawning.teams[team]);
        else if (op === 'insert') made = game.ai.spawning.insert(team, false, -1, name);
        else if (op === 'newHuman') { made = new Character(game.battle.characters.length, team, true); game.battle.characters.push(made); }
        else if (op === 'removeBody') unit.remove();
        else if (op === 'retire') {
          if (unit.seat?.vehicle) game.vehicles.unboard(unit, { force: true, placement: false });
          else if (unit.seat?.turret) game.turretSeats.leave(unit, { placement: false });
          unit.human = unit.networkHuman = false;
          unit.combat.player = false;
          unit.onRemoved = u => game.factory.release(u);
          unit.remove();
        }
      } finally { owner.feed(); }
      return { character: made, name: named, players: current ? players : null };
    },
  });
  function run(op, args = {}) {
    owner.feed();
    bridge.callArgs(input, { op, classic, players, armySize, hasAi: Boolean(game.ai), local: game.battle.human ?? null, character: null, team: 0, playerId: undefined, name: '', ...args });
    const result = bridge.callResult(output, x.roster_run());
    players = result.players;
    owner.sync();
    if (result.full) throw Error('team full');
    return result;
  }
  run('create');
  return {
    reserve(team, id, name) { return run('reserve', { team, playerId: id, name: String(name) }).character; },
    release(character) { run('release', { character }); },
    switchTeam(character, team) { return run('switchTeam', { character, team }).flag; },
    counts() { return run('counts').counts.map(c => classic ? { team: c.team, humans: c.humans, total: c.total } : { team: c.team, humans: c.humans }); },
  };
}
