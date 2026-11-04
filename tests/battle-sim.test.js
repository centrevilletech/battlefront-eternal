import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Battle, Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { attachBattleSim } from '../src/battle-sim.js';
import { attachPostsSim } from '../src/posts-sim.js';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, exportState } from '../src/sim/runtime.js';

async function pair() {
  const make = async rust => {
    const posts = new CommandPosts(null, { commandPosts: [
      { name: 'cp1', team: 1, matrix: new THREE.Matrix4().toArray(), hologram: [0, 0, 0], valueBleed: 3 },
      { name: 'cp2', team: 2, matrix: new THREE.Matrix4().toArray(), hologram: [0, 0, 0], valueBleed: 9 },
    ] }, { headless: true, playerTeam: 1 });
    const events = [];
    const battle = new Battle({ posts, teams: {
      1: { units: 2, reinforcements: 100, bleed: [[4, 4], [10, 1]] },
      2: { units: 2, reinforcements: 100 },
      3: { units: 1, reinforcements: 10 },
    }, own: 1, foe: 2, relation: (a, b) => a === b || a === 3 && b === 1 ? 1 : -1,
      living: () => 2,
      onOver(loser) {
        events.push([loser, battle.over, battle.loser, battle.countdown, battle.characters[0].record?.slots[0].life]);
        battle.spend(1, 3);
        battle.point(battle.characters[0]);
      },
    });
    battle.characters = [new Character(0, 1), new Character(1, 2)];
    for (const [i, c] of battle.characters.entries()) {
      c.className = `class${i}`;
      c.unit = { id: i, alive: true, removed: false, position: new THREE.Vector3(i, 2, 3), classDef: { displayName: `Soldier${i}` },
        combat: { dead: false, health: 100, maxHealth: 100, arsenal: { current: () => ({ name: `weapon${i}`, def: { displayName: `Blaster${i}` } }) } } };
    }
    if (rust) {
      const instance = await loadSim();
      attachPostsSim(instance, posts);
      attachBattleSim(instance, battle);
    }
    const snapshot = () => {
      const cx = new ExportContext({ ref: (kind, value) => ({ kind, id: value.id ?? value.index ?? value.name }) });
      return { battle: exportState('battle', battle, cx), characters: battle.characters.map(c => exportState('character', c, cx)) };
    };
    return { battle, posts, events, snapshot };
  };
  const js = await make(false), rust = await make(true);
  return { js, rust, step(fn) {
    const a = fn(js), b = fn(rust);
    assert.deepEqual(b, a);
    assert.deepEqual(rust.snapshot(), js.snapshot());
    assert.deepEqual(rust.events, js.events);
  } };
}

test('native battle tallies, tickets and ring order match the JavaScript rules', async () => {
  const { step } = await pair();
  step(({ battle }) => battle.update(1 / 60));
  for (let i = 0; i < 110; i++) {
    step(({ battle }) => battle.credit(battle.characters[1], battle.characters[0], i % 5 === 0 ? 1 : -1));
    step(({ battle }) => battle.update(i % 3 === 0 ? 0.01 : 0.2));
  }
  step(({ battle }) => battle.headshot(battle.characters[0]));
  step(({ battle }) => battle.vehicleDestroyed(battle.characters[0]));
  step(({ battle }) => battle.postDestroyed(battle.characters[1], -1));
  step(({ battle }) => battle.point(undefined));
  step(({ battle }) => battle.credit(null, null));
  step(({ battle }) => battle.spend(7));
});

test('native battle reports an ended match before reentrant callbacks run', async () => {
  const { step } = await pair();
  step(({ battle }) => { battle.spend(2, 100); battle.update(1 / 60); });
  for (const dt of [1, 1, 7, NaN]) step(({ battle }) => battle.update(dt));
  step(({ battle }) => battle.credit(battle.characters[1], battle.characters[0], -1));
});

test('native post scores preserve neutral posts, locals and countdown rules', async () => {
  const { step } = await pair();
  step(({ battle }) => battle.score(1));
  step(({ battle, posts }) => { posts.posts[0].team = 0; return battle.score(1); });
  step(({ battle, posts }) => { posts.posts[0].active = false; battle.updateTeam(1, 20); });
  step(({ battle }) => battle.updateLocals(3));
  step(({ battle }) => battle.update(0));
});
