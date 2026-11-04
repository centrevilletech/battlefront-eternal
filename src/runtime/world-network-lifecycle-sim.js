import { entitySpawnPosts } from './entity-spawn-posts.js';
import { choosePost, spawnPointAt, RESPAWN_DELAY } from '../human-spawn.js';

const ports = new WeakMap();

// Object identities are local to an invocation, including nested operations
// on the same player. Imports read only the stage the native driver requests.
export function createNetworkLifecycleBridge(bridge) {
  if (ports.has(bridge)) return ports.get(bridge);
  const contexts = [], x = bridge.x;
  bridge.register({ networkLifecycle(args) {
    const op = args.op, value = args.value, c = contexts.at(-1);
    const { game, player: p, unit, previous, className, remoteWorld } = c;
    let flag = false, other = false, number = 0, second = 0;
    switch (op) {
      case 'battleOver': flag = Boolean(game.battle.gameOver); break;
      case 'roster': flag = Boolean(game.spawnPort.roster(p.team).some(x => x.className === className)); break;
      case 'current': flag = Boolean(p.unit); break;
      case 'alive': flag = Boolean(p.unit.alive); break;
      // Addition precedes comparison in the source, including its coercion.
      case 'deadTime': number = +(p.unit.deadTime + 1e-6); break;
      case 'post': {
        const posts = entitySpawnPosts(game);
        c.post = choosePost(posts, p.team, posts.find(x => x.name === c.postName), c.relation, () => 0.5);
        flag = Boolean(c.post); break;
      }
      case 'prepared': flag = Boolean(game.factory.prepared(className)); break;
      case 'previous': c.previous = p.unit; flag = Boolean(c.previous); other = c.previous?.className === className; break;
      case 'reuse': c.unit = previous; break;
      case 'acquire': c.unit = game.factory.acquire(className); flag = c.unit === previous; break;
      case 'retireFlags': previous.human = previous.networkHuman = previous.combat.player = false; break;
      case 'retireHandler': previous.onRemoved = u => game.factory.release(u); break;
      case 'retire': previous.remove(); break;
      case 'unitFlags':
        unit.onRemoved = () => {};
        unit.human = true; unit.networkHuman = true;
        unit.networkShot = null; unit.networkShots = []; unit.networkViewTick = null;
        unit.networkInputLeadTicks = null; unit.networkPlayoutMaxTicks = null;
        break;
      case 'combatPlayer': unit.combat.player = true; break;
      case 'team': unit.team = unit.combat.team = p.team; break;
      case 'character': unit.character = p.character; break;
      case 'characterUnit': p.character.unit = unit; break;
      case 'characterClass': p.character.className = className; break;
      case 'characterReady': p.character.spawn.ready = false; break;
      case 'characterTimer': p.character.spawn.timer = Infinity; break;
      case 'previousAlive': flag = Boolean(previous?.alive); break;
      case 'teamExists': flag = Boolean(game.ai?.spawning?.teams?.[p.team]); break;
      case 'livingAdd': game.ai.spawning.teams[p.team].living++; break;
      case 'remoteClear': previous.combat.remotes = null; previous.remoteStep = null; break;
      // Keep assignment receivers evaluated before the callback's arguments.
      case 'remoteBind': unit.combat.remotes = remoteWorld.rebind(p.id, unit); break;
      case 'remoteStep': unit.remoteStep = (dt, input, controls) => remoteWorld.step(p.id, game.worldClock.tick + 1, dt, input, controls); break;
      case 'publish': p.unit = unit; p.className = className; break;
      case 'generations': {
        const a = unit.generation, b = previous.generation;
        number = +a; second = +b; break;
      }
      case 'generation': unit.generation = value; break;
      // These are unchanged human helper ports, not the AI spawn algorithm.
      case 'spawn': unit.spawn(...spawnPointAt(c.post, p.team, () => ((p.ordinal * 0.381966) % 1))); break;
      case 'weapons': {
        c.iterator = unit.combat.arsenal.weapons[Symbol.iterator]();
        if (Object(c.iterator) !== c.iterator) throw new TypeError('Iterator is not an object');
        c.next = c.iterator.next; break;
      }
      case 'nextWeapon': {
        c.body = false;
        const result = Reflect.apply(c.next, c.iterator, []);
        if (Object(result) !== result) throw new TypeError('Iterator result is not an object');
        if (!result.done) { c.weapon = result.value; c.body = true; flag = true; }
        break;
      }
      case 'weapon': c.weapon.random = c.networkWeapons; break;
      case 'spawnStatus':
        p.spawn.phase = 'alive'; p.spawn.spawnedOnce = true; p.spawn.className = className; p.spawn.post = c.post.name;
        break;
      case 'emit': game.simEvents.emit('spawn', { target: game.units.ref(unit), team: p.team, position: unit.position, className, post: c.post.name }); break;
      case 'unspawnUnit': c.unit = p.unit; break;
      case 'unspawnFlags':
        unit.alive = false; unit.dead = unit.combat.dead = unit.soldier.dead = true;
        unit.removed = true; unit.targetable = false;
        break;
      case 'invisible': unit.actor.root.visible = false; break;
      case 'unspawnTime': unit.deadTime = value; unit.combat.deathTime = 0; break;
      case 'livingRemove': game.ai.spawning.teams[p.team].living--; break;
      case 'unspawnStatus':
        p.spawn.phase = 'unspawned'; p.spawn.spawnedOnce = false; p.spawn.post = null; p.spawn.deadTime = unit.deadTime;
        break;
      case 'player': c.player = c.players.get(c.id); flag = Boolean(c.player); break;
      case 'disconnect': p.disconnectedAt = game.worldClock.tick + 1; break;
      case 'stop': { const stop = c.stopInput; stop(p); break; }
      case 'remoteExit': remoteWorld?.exit(p.id); break;
      case 'removeUnit': c.unit = p.unit; break;
      case 'vehicle': flag = Boolean(unit?.seat?.vehicle); break;
      case 'turret': flag = Boolean(unit?.seat?.turret); break;
      case 'unboard': game.vehicles.unboard(unit, { force: true, placement: false }); break;
      case 'leave': game.turretSeats?.leave(unit, { placement: false }); break;
      default: throw Error(`Unknown network lifecycle operation: ${op}`);
    }
    return { flag, other, value: number, second };
  } });
  function run(context, operation) {
    contexts.push(context);
    try { return operation(); }
    catch (error) {
      if (context.body) { try { context.iterator.return?.(); } catch {} }
      throw error;
    } finally { contexts.pop(); }
  }
  const api = {
    networkSpawn(context) {
      return run({ ...context }, () => Boolean(x.world_network_spawn(RESPAWN_DELAY, Boolean(context.remoteWorld), Boolean(context.networkWeapons))));
    },
    networkUnspawn(game, player) { run({ game, player }, () => x.world_network_unspawn(RESPAWN_DELAY)); },
    networkRemove(game, players, id, stopInput, remoteWorld) {
      run({ game, players, id, stopInput, remoteWorld }, () => x.world_network_remove());
    },
  };
  ports.set(bridge, api);
  return api;
}
