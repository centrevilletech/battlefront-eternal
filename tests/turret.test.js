import { rustOrdnance } from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { makeHit } from '../src/collision.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Gunner } from '../src/gunner.js';
import { loadSim } from '../src/sim/module.js';
import { SimBridge } from '../src/sim/runtime.js';
import { attachOrdnanceSim, ordnanceTests } from '../src/ordnance-sim.js';
import { KIND_BEAM, KIND_TABLE } from '../src/sim/gen/index.js';

function box(z, depth = .5) {
  const g = new THREE.BoxGeometry(4, 4, depth).translate(0, 1, z);
  return [g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3)];
}
function world() {
  const collision = new CollisionWorld(...box(10));
  const own = collision.addBody(...box(0, 3));
  return { collision, own };
}

test('turret ray ignores only its own live body', () => {
  const { collision, own } = world(), hit = makeHit();
  const origin = new THREE.Vector3(0, 1, -3), direction = new THREE.Vector3(0, 0, 1);
  assert.equal(collision.cast(origin, direction, 100, hit).body, own);
  assert.equal(collision.cast(origin, direction, 100, hit, own).distance, 12.75);
  const other = collision.addBody(...box(5));
  assert.equal(collision.cast(origin, direction, 100, hit, own).body, other);
});



// A gunner whose turret no lease holds (set by hand, as above) fires beams
// that follow the turret: its frame, its life and its gun's view. The
// rounds on the simulation's module name the turret's emitter by a handle
// and follow it as the JavaScript rounds do.


for (const map of ['nab2', 'bes2']) test(`${map} exports turret bolt sprites independent of player side`, { skip: !existsSync(`public/maps/${map}/props.json`) }, () => {
  const props = JSON.parse(readFileSync(`public/maps/${map}/props.json`));
  const manifest = JSON.parse(readFileSync(`public/maps/${map}/map.json`));
  for (const p of props.instances.filter(p => p.weapon?.ordnance?.LaserTexture)) {
    const name = p.weapon.ordnance.LaserTexture;
    assert.ok(manifest.textures[name], `${map}: ${name} is a map dependency`);
    assert.ok(existsSync(`public/maps/${map}/${manifest.textures[name]}`));
  }
});
