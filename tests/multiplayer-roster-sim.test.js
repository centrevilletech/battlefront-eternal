import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Battle, Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { ClassicSpawning } from '../src/ai/classic/spawning.js';
import { EnhancedSpawning } from '../src/ai/enhanced/objectives.js';
import { ClassicRandom } from '../src/ai/random.js';
import { attachBattleSim } from '../src/battle-sim.js';
import { attachPostsSim } from '../src/posts-sim.js';
import { attachSpawningSim } from '../src/spawning-sim.js';
import { createMultiplayerRoster } from '../src/runtime/multiplayer-roster.js';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, exportState } from '../src/sim/runtime.js';

async function pair(mode = 'classic', size = 3) {
  async function make(rust) {
    const posts = new CommandPosts(null, { commandPosts: [1, 2, 1].map((team, i) => ({ name: `cp${i}`, team,
      matrix: new THREE.Matrix4().makeTranslation(i * 50, 0, 0).toArray(), hologram: [0, 0, 0] })) }, { headless: true, playerTeam: 1 });
    for (const p of posts.posts) p.ai = { spawnPath: { present: true, points: [[p.position.x, 0, 0, 0]] },
      allyPath: null, allyCount: 0, classSpawnPoints: 0, classSpawnWorld: [], yaw: 0 };
    const battle = new Battle({ posts, teams: { 1: { units: 3 }, 2: { units: 2 } }, own: 1, foe: 2 });
    const random = new ClassicRandom(739), trace = [];
    const game = { battle, posts, ai: null,
      factory: { release(u) { trace.push(['released', u.id]); } },
      vehicles: { unboard(u, opts) { trace.push(['unboard', u.id, opts.force, opts.placement]); u.seat = null; } },
      turretSeats: { leave(u, opts) { trace.push(['leave', u.id, opts.placement]); u.seat = null; } },
    };
    if (mode === 'off' || mode === 'standin') {
      battle.populate(1);
      if (mode === 'standin') game.ai = { update() {} };
    } else {
      const Spawning = mode === 'enhanced' ? EnhancedSpawning : ClassicSpawning;
      const spawning = new Spawning({ random, posts: () => posts.all(), humans: [{ team: 1 }],
        calls: [['SetUnitCount', [1, 3]], ['SetUnitCount', [2, 2]], ['AddUnitClass', [1, 'a', 2]], ['AddUnitClass', [1, 'b', 1]], ['AddUnitClass', [2, 'c', 2]]],
        makeCharacter: (slot, team, human) => new Character(slot, team, human),
        objectives: { request(c) { c.objective = posts.posts[2]; }, republish() {} },
      });
      battle.characters = spawning.characters;
      game.ai = { spawning, classic: {
        attach(c) { trace.push(['attach', c.team, c.index, c.human, c.className]); battle.point(c); },
        detach(c) { trace.push(['detach', c.team, c.index, c.human, Boolean(c.unit)]); },
      } };
      for (const [i, c] of battle.characters.entries()) {
        if (i > 4 || c.human) continue;
        c.unit = { id: i, alive: true, human: true, networkHuman: true, seat: i % 2 ? { turret: {} } : { vehicle: {} },
          position: new THREE.Vector3(), combat: { player: true, health: 100, dead: false }, remove() {
            trace.push(['remove', this.id, this.human, this.networkHuman, this.combat.player, this.seat]);
            this.alive = false; this.onRemoved?.(this);
          } };
        spawning.teams[c.team].living++;
      }
    }
    if (rust) {
      const instance = await loadSim();
      game.postsSim = attachPostsSim(instance, posts);
      game.battleSim = attachBattleSim(instance, battle);
      if (game.ai?.spawning) game.spawningSim = attachSpawningSim(game.ai.spawning, game.battleSim, game.postsSim);
    }
    const originalList = battle.characters;
    const roster = createMultiplayerRoster(game, size);
    assert.equal(battle.characters, originalList);
    const snapshot = () => {
      const cx = new ExportContext({ classes: { Post: posts.posts[0].constructor }, ref: (kind, value) => ({ kind, id: kind === 'character' ? battle.characters.indexOf(value) : value.id ?? value.name }) });
      return { battle: exportState('battle', battle, cx), characters: battle.characters.map(c => exportState('character', c, cx)),
        spawning: game.ai?.spawning ? exportState('spawning', game.ai.spawning, cx) : null, random: random.seed, trace: structuredClone(trace), counts: roster.counts() };
    };
    return { game, battle, roster, snapshot, players: [] };
  }
  const js = await make(false), rust = await make(true);
  assert.deepEqual(rust.snapshot(), js.snapshot());
  return { js, rust, step(fn) { assert.deepEqual(fn(rust), fn(js)); assert.deepEqual(rust.snapshot(), js.snapshot()); } };
}

for (const mode of ['classic', 'enhanced']) test(`native ${mode} roster preserves bot replacement, names, queues and callback order`, async () => {
  const { step } = await pair(mode);
  for (let i = 0; i < 45; i++) {
    step(p => { const c = p.roster.reserve(i % 2 + 1, `connection${i}`, `${'🙂'.repeat(20)}${i}`); p.players.push(c); return p.battle.characters.indexOf(c); });
    step(p => p.roster.switchTeam(p.players.at(-1), (i + 1) % 2 + 1));
    step(p => p.roster.release(p.players.pop()));
  }
  step(p => { for (let i = 0; i < 3; i++) p.players.push(p.roster.reserve(1, i, 'Player')); });
  step(p => { assert.throws(() => p.roster.reserve(1, 'extra', 'Extra'), /team full/); });
  step(p => { const c = p.roster.reserve(2, 'other', 'Other'); return p.roster.switchTeam(c, 1); });
});

for (const mode of ['off', 'standin']) test(`native ${mode} roster reuses vacant slots and preserves capacity`, async () => {
  const { step } = await pair(mode, 2);
  step(p => { p.players.push(p.roster.reserve(1, 'stay', 'Stays')); });
  for (let i = 0; i < 500; i++) {
    step(p => { const c = p.roster.reserve(i % 2 + 1, i, 'Leaves'); c.kills = 7; c.deaths = 2; p.roster.release(c); });
  }
  step(p => { p.players.push(p.roster.reserve(1, 'full', 'Full')); assert.throws(() => p.roster.reserve(1, 'extra', 'Extra'), /team full/); });
  step(p => { const c = p.roster.reserve(2, 'other', 'Other'); return p.roster.switchTeam(c, 1); });
});

test('native Enhanced spawning chooses the nearest friendly post before the Classic fallback', async () => {
  const { step } = await pair('enhanced');
  step(p => {
    const s = p.game.ai.spawning, c = s.characters.find(c => c.team === 1 && !c.human);
    c.spawn.post = p.game.posts.posts[0];
    c.objective = p.game.posts.posts[2];
    const chosen = s.choosePost(c);
    assert.equal(chosen, p.game.posts.posts[2]);
    return chosen.name;
  });
  step(p => {
    const s = p.game.ai.spawning, c = s.characters.find(c => c.team === 1 && !c.human);
    p.game.posts.posts[2].active = false;
    return s.choosePost(c).name;
  });
  step(p => {
    const s = p.game.ai.spawning, c = s.characters.find(c => c.team === 1 && !c.human);
    c.objective = null;
    return s.choosePost(c).name;
  });
});

test('native player removal retains a nested reservation and its array identity', async () => {
  const { step } = await pair('off', 3);
  step(p => {
    const c = p.roster.reserve(1, 'leaving', 'Leaves');
    c.unit = { id: 999, remove() { p.players.push(p.roster.reserve(2, 'nested', 'Joins')); } };
    p.roster.release(c);
  });
  step(p => p.roster.release(p.players.pop()));
});
