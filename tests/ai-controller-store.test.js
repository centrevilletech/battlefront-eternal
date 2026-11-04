import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Character } from '../src/battle.js';
import { CommandPosts } from '../src/commandposts.js';
import { MobilePost } from '../src/mobile-posts.js';
import { SourceGoal, UnitController } from '../src/ai/classic/controller.js';
import { ClassicAI } from '../src/ai/classic/index.js';
import { ClassicRandom } from '../src/ai/random.js';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, Identity, SimBridge, exportState } from '../src/sim/runtime.js';
import { KIND_CHARACTER, KIND_CLASSIC_AI, KIND_CONTROLLER, KIND_MOBILE_POST, KIND_POST, KIND_SEAT_UNIT, KIND_TABLE, KIND_UNIT } from '../src/sim/gen/index.js';

test('AI goals preserve placed and mobile command post identities through the store', async () => {
  const posts = new CommandPosts(null, { commandPosts: [{
    name: 'placed', team: 1, matrix: new THREE.Matrix4().toArray(), hologram: [0, 0, 0],
  }] }, { headless: true, playerTeam: 1 });
  const placed = posts.posts[0];
  const mobile = new MobilePost(null, {}, 0, {});
  const ai = new ClassicAI({ randomA: new ClassicRandom(1), randomB: new ClassicRandom(2) });
  const source = new UnitController(ai, new Character(0, 1));
  ai.controllers.push(source);
  const bridge = new SimBridge(await loadSim());
  bridge.setRefs(KIND_POST, new Identity(KIND_POST, { test: value => value === placed }));
  bridge.setRefs(KIND_MOBILE_POST, new Identity(KIND_MOBILE_POST, { test: value => value instanceof MobilePost }));

  for (const current of [mobile, placed, null, undefined]) {
    source.commands.current = SourceGoal.objective(current);
    // The union also keeps undefined distinct from null when reading an
    // existing queue, even though SourceGoal.objective normally uses null.
    source.commands.current.post = current;
    source.commands.queue = [SourceGoal.objective(placed), SourceGoal.objective(mobile), SourceGoal.objective(null)];
    bridge.write(KIND_CONTROLLER, [source], true);
    bridge.write(KIND_CLASSIC_AI, [ai], true);
    const [{ object }] = bridge.read(KIND_CONTROLLER);
    assert.equal(KIND_TABLE[KIND_CONTROLLER].same(source, object), null);
    assert.equal(object.commands.current.post, current);
    assert.deepEqual(object.commands.queue.map(goal => goal.post), [placed, mobile, null]);
    assert.equal(bridge.refs[KIND_POST].ids.has(mobile), false);
    assert.equal(bridge.refs[KIND_MOBILE_POST].ids.has(placed), false);
    assert.equal(bridge.refs[KIND_MOBILE_POST].ids.has(mobile), true);

    const exported = exportState('controller', source, new ExportContext({
      classes: { MobilePost, Post: placed.constructor }, ref: (kind, value) => ({ kind, name: value.name ?? null }),
    }));
    assert.equal(exported.commands.queue[0].post.kind, 'post');
    assert.equal(exported.commands.queue[1].post.kind, 'mobilePost');
    assert.equal(exported.commands.queue[2].post, null);
  }
});

test('AI shot alerts preserve Character and direct unit sources through the store', async () => {
  const ai = new ClassicAI({ randomA: new ClassicRandom(1), randomB: new ClassicRandom(2) });
  const character = new Character(0, 1);
  const source = new UnitController(ai, character);
  ai.controllers.push(source);
  const unit = { id: 0, generation: 3, alive: true, position: new THREE.Vector3() };
  const seat = { generation: 3, alive: true, position: new THREE.Vector3() };
  const bridge = new SimBridge(await loadSim());
  bridge.setRefs(KIND_CHARACTER, new Identity(KIND_CHARACTER, { test: value => value === character }));
  bridge.setRefs(KIND_UNIT, new Identity(KIND_UNIT, { test: value => value === unit }));
  bridge.setRefs(KIND_SEAT_UNIT, new Identity(KIND_SEAT_UNIT, { test: value => value === seat }));
  for (const attacker of [character, unit, seat, null, undefined]) {
    source.shot.source = attacker;
    source.shot.pending = true;
    source.shot.known = true;
    source.shot.point.set(-0, 1, 2);
    bridge.write(KIND_CONTROLLER, [source], true);
    bridge.write(KIND_CLASSIC_AI, [ai], true);
    const [{ object }] = bridge.read(KIND_CONTROLLER);
    assert.equal(KIND_TABLE[KIND_CONTROLLER].same(source, object), null);
    assert.equal(object.shot.source, attacker);
    assert.ok(Object.is(object.shot.point.x, -0));
  }
});
