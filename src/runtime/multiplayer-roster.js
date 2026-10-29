import { Character } from '../battle.js';
import { MAX_TEAM_SIZE } from '../battle-limits.js';
// Network connections reserve army slots, replacing bots without spending a
// reinforcement. Character array positions remain stable for replication.
export function createMultiplayerRoster(game, armySize = MAX_TEAM_SIZE) {
  if (armySize !== null && (!Number.isInteger(armySize) || armySize < 1 || armySize > MAX_TEAM_SIZE)) throw new Error('invalid army size');
  const spawning = game.ai?.spawning, classic = game.ai?.classic;
  if (!spawning || !classic) {
    // With the AI off nobody fields the mission's armies: the bots
    // Battle.populate made never spawn, yet the roster replicates every
    // slot, so each was a row on every Tab table (49 on a Cloud City duel
    // server). The roster holds the players alone, from before anyone
    // joins. The stand-in AI fields its own, so they stay with it.
    if (!game.ai) { const characters=game.battle.characters; characters.splice(0,characters.length,...characters.filter(c=>c.human)); }
    const reserved=new Set();
    const capacity=armySize??MAX_TEAM_SIZE;
    const full=team=>[...reserved].filter(c=>c.team===team).length>=capacity;
    return {
      // A slot a player left is taken again, with fresh scores: the roster
      // replicates every slot (at most 2,400), and a battle without bots
      // must not grow with every join on a server that runs for days.
      reserve(team,id,name){if(full(team))throw Error('team full');let c=game.battle.characters.find(c=>c.human&&c.team===0&&c.playerId!==undefined&&!reserved.has(c));
        if(c){c.team=team;c.kills=c.teamKills=c.deaths=c.captures=0;c.record=null;c.objective=null;c.className=null;}else{c=new Character(game.battle.characters.length,team,true);game.battle.characters.push(c);}
        c.name=String(name).slice(0,32);c.playerId=id;c.spawn={ready:false,timer:Infinity};reserved.add(c);return c;},
      release(c){c.unit?.remove();c.unit=null;c.team=0;reserved.delete(c);},
      switchTeam(c,team){if(c.team!==team&&full(team))return false;c.team=team;return true;},
      counts(){return [1,2].map(team=>({team,humans:[...reserved].filter(c=>c.team===team).length}));}
    };
  }
  function retire(character) {
    spawning.changePost(character, null);
    classic.detach(character);
    const unit = character.unit;
    if (unit) {
      if (unit.alive && !character.hero) spawning.teams[character.team].living--;
      if (unit.seat?.vehicle) game.vehicles.unboard(unit, { force:true, placement:false });
      else if (unit.seat?.turret) game.turretSeats.leave(unit, { placement:false });
      unit.human = unit.networkHuman = false;
      unit.combat.player = false;
      unit.onRemoved = u => game.factory.release(u);
      unit.remove();
    }
    character.unit = null;
  }
  function reset(character) {
    character.kills = character.teamKills = character.deaths = character.captures = 0;
    character.record = null;
    character.objective = null;
  }
  function bot(character) {
    character.human = false;
    character.playerId = -1;
    character.name = spawning.botName(spawning.teams[character.team]);
    reset(character);
    character.spawn.timer = 0;
    spawning.activate(character);
    classic.attach(character);
  }
  const dormant = game.battle.human;
  if (dormant) {
    retire(dormant);
    dormant.networkDormant = true;
    dormant.spawn.ready = false;
    dormant.spawn.timer = Infinity;
    const team = spawning.teams[dormant.team];
    team.characters.splice(team.characters.indexOf(dormant), 1);
  }
  for (const number of [1,2]) {
    const team = spawning.teams[number];
    // Local play keeps the mission's population (including battle-size
    // scaling), not ClassicSpawning's 200-character allocation ceiling.
    const capacity = armySize ?? Math.min(MAX_TEAM_SIZE, Math.max(1, team.unitCount));
    while (team.characters.length > capacity) {
      const character = team.characters.pop();
      retire(character);
      character.networkDormant = true;
      character.human = true;
      character.spawn.ready = false;
      character.spawn.timer = Infinity;
    }
    team.capacity = team.unitCount = capacity;
    while (team.characters.length < capacity) {
      const index = Math.max(-1, ...team.characters.map(c => c.index)) + 1;
      const character = spawning.insert(number, false, -1, spawning.botName(team));
      character.index = character.slot = index;
      spawning.activate(character);
      classic.attach(character);
    }
    spawning.padQuotas(team);
  }
  const available = team => spawning.teams[team].characters
    .filter(c => !c.human).sort((a,b) => Number(!!a.unit?.alive) - Number(!!b.unit?.alive))[0];
  return {
    reserve(team, id, name) {
      const character = available(team);
      if (!character) throw new Error('team full');
      retire(character);
      reset(character);
      character.human = true;
      character.hero = false;
      character.playerId = id;
      character.name = String(name).slice(0,32);
      character.spawn.ready = false;
      character.spawn.timer = Infinity;
      return character;
    },
    release(character) { retire(character); bot(character); },
    switchTeam(character, target) {
      if (target === character.team) return true;
      const replacement = available(target);
      if (!replacement) return false;
      const previous = character.team;
      const source = spawning.teams[previous].characters, destination = spawning.teams[target].characters;
      retire(replacement);
      spawning.changePost(character, null);
      source[source.indexOf(character)] = replacement;
      destination[destination.indexOf(replacement)] = character;
      character.team = target;
      replacement.team = previous;
      bot(replacement);
      return true;
    },
    counts() { return [1,2].map(team => ({team, humans:spawning.teams[team].characters.filter(c=>c.human).length, total:spawning.teams[team].characters.length})); },
  };
}
