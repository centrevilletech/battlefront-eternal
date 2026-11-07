import { Matrix4 } from 'three';
import { CommandPosts } from '../../src/commandposts.js';
import { F4Live } from '../../src/sim/wasm/f4-live.js';
import { F4Tail } from '../../src/sim/wasm/f4-tail.js';
import { HumanSpawnMirror } from '../../src/sim/gen/index.js';
import { FixedClock } from '../../src/runtime/fixed-clock.js';
import { WorldTick } from '../../src/runtime/world-tick.js';
import { createMultiplayerWorld } from '../../src/runtime/multiplayer-world.js';
import { RandomStreams } from '../../src/ai/random.js';
import { SimEvents } from '../../src/sim-events.js';
import { f3World } from './f3-world.js';
import { battleFixture } from './f4-battle.js';

// The production admission host and enclosing native tick over synthetic
// classes and posts. No game files, player policy or queue twin is needed.
export async function playersWorld({ fleet = null, definitions = { soldier: { class: 'soldier', base: 'soldier', MaxHealth: 100, weapons: [] } }, unitOptions = {} } = {}) {
  const posts = new CommandPosts(null, { commandPosts: [1, 2].map(team => ({
    name: `post${team}`, team, matrix: new Matrix4().makeTranslation(team * 20, 0, 0).toArray(), hologram: [0, 0, 0],
  })) }, { headless: true, playerTeam: 1 });
  for (const post of posts.posts) post.ai = { spawnPath: { present: true, points: [[post.position.x, 0, 0, 0]] },
    allyPath: null, allyCount: 0, classSpawnPoints: 0, classSpawnWorld: [], yaw: 0 };
  const battle = battleFixture(posts), character = battle.characters[0];
  const names = Object.keys(definitions), initialClass = names[0];
  const f = await f3World(definitions, { debug: false, prepareFleet: fleet,
    prepareLive: () => ({ buildings: [], animated: [], traps: [] }),
    prepareUnit: unit => { unit.team = unit.combat.team = 1; }, ...unitOptions,
  });
  const unit = f.spawn(initialClass, 0, 0, true, null);
  character.unit = unit; character.className = initialClass; unit.character = character;
  f.phase.registerViewpoint(unit);
  new F4Live(f.phase, { local: unit, sound() {} });
  const human = new HumanSpawnMirror();
  const tail = new F4Tail(f.phase, battle, { posts, human, local: unit, character,
    gameplay: f.ordnance.random, rosters: [[], names, names], classes: names });
  const clock = new FixedClock(), tick = new WorldTick(f.phase, clock), streams = new RandomStreams(7);
  f.factory.prepare = async () => {};
  const game = { battle, posts, factory: f.factory, units: f.units, ordnance: f.ordnance,
    vehicles: f.factory.context.vehicles, unitsSim: f.phase, simBridge: f.phase.bridge, playerUnit: unit, tailOwner: tail,
    separationScope: f.separation, simEvents: new SimEvents(), worldClock: clock,
    spawnPort: { roster: () => names.map(className => ({ className })) },
    randomStream: name => streams.draw(name),
    step(command) {
      tick.run(1 / 60, command.tick, true, null, false, true, 0, false, 0, command.at, 1);
      game.beforeWorldFinish?.();
      f.phase.eventPrefix = tick.output.playersEvents;
      f.phase.eventClock = early => { game.simEvents.time = (command.tick - (early ? 1 : 0)) / 60; };
      tick.finish(); f.phase.eventClock = null; tick.complete();
    },
  };
  const world = createMultiplayerWorld(game);
  return { game, world, tick, phase: f.phase, spawn: f.spawn, factory: f.factory, units: f.units, ordnance: f.ordnance, items: f.items };
}

export const playerCommand = (actor, tick, extra = {}) => ({ epoch: 1, generation: actor.unit.generation,
  sequence: tick, tick, down: [], actions: [], look: [], ...extra });
