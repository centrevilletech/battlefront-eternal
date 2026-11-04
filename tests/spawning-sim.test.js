import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Battle, Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { ClassicSpawning } from '../src/ai/classic/spawning.js';
import { ClassicRandom } from '../src/ai/random.js';
import { attachBattleSim } from '../src/battle-sim.js';
import { attachPostsSim } from '../src/posts-sim.js';
import { attachSpawningSim } from '../src/spawning-sim.js';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, exportState } from '../src/sim/runtime.js';

async function pair() {
  const make = async rust => {
    const posts = new CommandPosts(null, { commandPosts: [1, 2].map(team => ({ name: `cp${team}`, team,
      matrix: new THREE.Matrix4().makeTranslation(team * 10, 0, 0).toArray(), hologram: [0, 0, 0] })) }, { headless: true, playerTeam: 1 });
    for (const p of posts.posts) p.ai = {
      spawnPath: { present: true, points: [[p.team * 10, 0, 0, 0], [p.team * 10, 0, 1, 0.5]] },
      allyPath: null, allyCount: 0, classSpawnPoints: 0, classSpawnWorld: [], yaw: 0,
    };
    const battle = new Battle({ posts, teams: { 1: { units: 3, reinforcements: 100 }, 2: { units: 2, reinforcements: 100 } }, own: 1, foe: 2 });
    const trace = [], random = new ClassicRandom(79);
    let spawning, nextId = 0, reenter = false;
    spawning = new ClassicSpawning({ battle, posts: () => posts.all(), random, humans: [{ team: 1 }],
      calls: [['SetUnitCount', [1, 3]], ['SetUnitCount', [2, 2]], ['AddUnitClass', [1, 'a', 2]], ['AddUnitClass', [1, 'b', 1]], ['AddUnitClass', [2, 'c', 2]], ['SetSpawnDelay', [1, 0.25]]],
      makeCharacter(slot, team, human) { const c = new Character(battle.characters.length, team, human); battle.characters.push(c); return c; },
      spawnUnit(c, name, post, point, yaw) {
        trace.push(['spawn', c.index, name, post.name, [...point], yaw, c.spawn.timer, spawning.teams[c.team].living]);
        c.unit = { id: nextId++, alive: true, position: new THREE.Vector3(...point), combat: { health: 100, maxHealth: 100, dead: false } };
        c.className = name;
        battle.point(c);
        return c.unit;
      },
      forceKill(c) { trace.push(['kill', c.index, c.spawn.timer]); spawning.onDeath(c); },
      objectives: {
        request(c) {
          trace.push(['request', c.index, c.className, c.spawn.timer]);
          c.objective = posts.posts[c.team - 1];
          if (reenter) { battle.point(c); spawning.changePost(c, posts.posts[0]); }
        },
        republish(c) { trace.push(['republish', c.index, c.className, c.spawn.ordinal]); },
      },
    });
    reenter = true;
    if (rust) {
      const instance = await loadSim(), ps = attachPostsSim(instance, posts), bs = attachBattleSim(instance, battle);
      attachSpawningSim(spawning, bs, ps);
    }
    const snapshot = () => {
      const cx = new ExportContext({ classes: { Post: posts.posts[0].constructor }, ref: (kind, v) => ({ kind, id: kind === 'character' ? v.index : v.id ?? v.name }) });
      return { battle: exportState('battle', battle, cx), spawning: exportState('spawning', spawning, cx), characters: battle.characters.map(c => exportState('character', c, cx)), seed: random.seed, trace: structuredClone(trace) };
    };
    return { spawning, posts, battle, snapshot };
  };
  const js = await make(false), rust = await make(true);
  return { js, rust, step(fn) {
    assert.deepEqual(fn(rust), fn(js));
    assert.deepEqual(rust.snapshot(), js.snapshot());
  } };
}

test('native spawn waves, draws and nested spawn/death callbacks match', async () => {
  const { step } = await pair();
  step(({ spawning, posts }) => spawning.onHumanReady(spawning.human, { classIndex: 1, post: posts.posts[0] }));
  for (let i = 0; i < 130; i++) step(({ spawning, battle }) => { spawning.clock(1 / 60); battle.update(1 / 60); spawning.updateCharacters(1 / 60); });
  step(({ spawning }) => spawning.onDeath(spawning.characters.find(c => !c.human && c.unit)));
  for (let i = 0; i < 70; i++) step(({ spawning }) => spawning.update(0.1));
});

test('native class changes, post queues and team transfers preserve identities', async () => {
  const { step } = await pair();
  step(({ spawning, posts }) => spawning.reserve(posts.posts[0], false, true));
  step(({ spawning, posts }) => spawning.reserve(posts.posts[0], true, true));
  step(({ spawning, posts }) => spawning.release(posts.posts[0], false, true));
  step(({ spawning, posts }) => { const q = spawning.queueOf(posts.posts[0]); spawning.reserve(posts.posts[0], false, true); assert.equal(spawning.queueOf(posts.posts[0]), q); return [...q]; });
  step(({ spawning }) => spawning.switchTeam(spawning.human, 2));
  step(({ spawning }) => spawning.setClass(spawning.human, 0));
  step(({ spawning }) => spawning.onHumanReady(spawning.human, { className: '' }));
  step(({ spawning }) => spawning.awaitingHuman(spawning.human));
  step(({ spawning }) => spawning.pickClass(spawning.characters[1]));
});

test('native hero removal can reenter the spawn manager and battle rules', async () => {
  const { step } = await pair();
  for (let i = 0; i < 25; i++) step(({ spawning }) => spawning.update(0.1));
  step(({ spawning, battle }) => {
    const c = spawning.characters.find(c => !c.human && c.unit);
    c.hero = true;
    battle.reinforcements[c.team] = 0;
    spawning.updateCharacter(c, 1 / 60);
  });
});
