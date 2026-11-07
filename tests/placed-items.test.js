import { rustItems } from './helpers/rust-simulation.js';
// A map's placed power-ups (src/placed-items.js, tools/zero_placed_items.py):
// the world makes each an ordinary item where the map puts it, facing its
// instance's way; tools/placed-items-check.mjs runs one in a headless match.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { spawnPlacedItems } from '../src/placed-items.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';

const health100 = { class: 'com_item_powerup_health100', base: 'powerupitem', Gravity: 1, Rebound: 0, Friction: 1,
  GeometryName: 'item_powerup_health100', PersonHealth: 75, DroidHealth: 75, LifeSpan: 30 };

function world() {
  const g = new THREE.PlaneGeometry(200, 200).rotateX(-Math.PI / 2);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  return rustItems({ scene: new THREE.Scene(), collision, effects: { play() {} }, lights: { add() {} }, model: () => null, explode() {} });
}

test('each placed power-up is an item from the start, where and as the map places it', () => {
  const items = world();
  // Turned 90 degrees about y, 2 m over the ground.
  const matrix = new THREE.Matrix4().makeRotationY(Math.PI / 2).setPosition(3, 2, -4).toArray();
  const made = spawnPlacedItems(items, [{ class: health100.class, name: '', team: 0, matrix, item: health100 }, { class: 'broken' }]);
  assert.equal(made.length, 1);
  const [item] = made;
  assert.equal(item.def, health100, 'the definition is the prepared object (the combat registry names it)');
  assert.deepEqual(item.position.toArray(), [3, 2, -4]);
  assert.ok(Math.abs(item.yaw - Math.PI / 2) < 1e-6);
  assert.equal(item.life, 30);
  assert.deepEqual(spawnPlacedItems(items,undefined),[]);
});
