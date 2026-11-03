import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld, makeHit } from '../src/collision.js';
import { Gunner } from '../src/gunner.js';
import { Ordnance } from '../src/ordnance.js';
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

test('turret aim passes its own body and its visible bolt survives firing', () => {
  const { collision, own } = world();
  const texture = new THREE.DataTexture(new Uint8Array([255, 20, 5, 255]), 1, 1);
  const ordnance = new Ordnance({ scene: new THREE.Scene(), collision, effects: {play() {}}, lights: {add() {}}, texture: () => texture });
  const camera = new THREE.PerspectiveCamera();camera.position.set(0, 1, -3);
  const def = {base: 'bolt', Velocity: 80, LifeSpan: 1.5, LaserTexture: 'redlaser_D', LaserWidth: .15, LaserLength: 2};
  const turret = { bodies: {built: own}, facing: 0, yaw: 0, pitch: 0, yawVel: 0, turnRate: 1,
    rig: {root: new THREE.Group()}, steer() {}, viewDirection: out => out.set(0, 0, 1), aimOrigin: out => out.set(0, 1, -3),
    muzzle(position, direction) { position.set(0, 1, 1.5);direction?.set(0, 0, 1); },
    pose(dt, target) { this.target = target.clone(); },
    weapon: {def: {ordnance: def}, update: () => [{}]},
  };
  const soldier = Object.fromEntries(['position','previous','settled','velocity'].map(k => [k, new THREE.Vector3()]));
  const gunner = new Gunner({camera, collision, ordnance, animator: {gunner() {}}, clips: new Map()});
  gunner.turret = turret;
  gunner.update(1/60, soldier, {take: () => true, held: () => true});
  assert.ok(turret.target.z > 9, 'crosshair must aim at the wall beyond the turret');
  assert.equal(ordnance.bolts.length, 1);
  assert.ok(ordnance.bolts[0].life > .09, 'shot must leave its own body');
  assert.equal(ordnance.bolts[0].impact.body, null, 'static wall remains collidable');
  ordnance.update(1/60);
  assert.equal(ordnance.bolts.length, 1);
  assert.ok(ordnance.bolts[0].flown > 1, 'bolt flies forward');
  assert.ok([...ordnance.renderer.batches.values()].some(batch => batch.count > 0), 'core sprite is submitted');
});

// A gunner whose turret no lease holds (set by hand, as above) fires beams
// that follow the turret: its frame, its life and its gun's view. The
// rounds on the simulation's module name the turret's emitter by a handle
// and follow it as the JavaScript rounds do.
test('a lease-less gunner\'s beam follows his turret, on the module as in JavaScript', async () => {
  const { collision, own } = world();
  const rounds = () => new Ordnance({ scene: new THREE.Scene(), collision, effects: {play() {}}, lights: {add() {}}, texture: () => null });
  const js = rounds(), rust = rounds();
  attachOrdnanceSim(rust, new SimBridge(await loadSim(), { tests: ordnanceTests }));
  const def = {class: 'beam', base: 'beam', LifeSpan: .4, Damage: 10};
  const view = new THREE.Vector3(0, 0, 1);
  const turret = { bodies: {built: own}, facing: 0, yaw: 0, pitch: 0, yawVel: 0, turnRate: 1, alive: true,
    matrix: new THREE.Matrix4().makeTranslation(0, 1, 0),
    rig: {root: new THREE.Group()}, steer() {}, viewDirection: out => out.copy(view), aimOrigin: out => out.set(0, 1, -3),
    muzzle(position, direction) { position.set(0, 1, 1.5);direction?.set(0, 0, 1); },
    pose(dt, target) { this.target = target.clone(); },
    weapon: {def: {ordnance: def}, update: () => [{}]},
  };
  const camera = new THREE.PerspectiveCamera();camera.position.set(0, 1, -3);
  for (const ordnance of [js, rust]) {
    const soldier = Object.fromEntries(['position','previous','settled','velocity'].map(k => [k, new THREE.Vector3()]));
    const gunner = new Gunner({camera, collision, ordnance, animator: {gunner() {}}, clips: new Map()});
    gunner.turret = turret;
    // The same spread draws for both.
    let draws = 0;
    gunner.random = () => [0.25, 0.75, 0.5][draws++ % 3];
    gunner.update(1/60, soldier, {take: () => true, held: () => true});
  }
  assert.equal(js.beams.length, 1);
  assert.equal(rust.beams.length, 1);
  assert.equal(KIND_TABLE[KIND_BEAM].same(js.beams[0], rust.beams[0]), null);
  // The turret rises and swings its gun round: each beam goes with it.
  turret.matrix.makeTranslation(0, 2, 0);
  view.set(1, 0, 1).normalize();
  js.update(1/60);
  rust.update(1/60);
  assert.equal(KIND_TABLE[KIND_BEAM].same(js.beams[0], rust.beams[0]), null);
  assert.deepEqual(js.beams[0].direction.toArray(), view.toArray());
  assert.equal(js.beams[0].position.y, 2);
  // A destroyed turret's beam goes out with it.
  turret.alive = false;
  js.update(1/60);
  rust.update(1/60);
  assert.equal(js.beams.length, 0);
  assert.equal(rust.beams.length, 0);
});

for (const map of ['nab2', 'bes2']) test(`${map} exports turret bolt sprites independent of player side`, { skip: !existsSync(`public/maps/${map}/props.json`) }, () => {
  const props = JSON.parse(readFileSync(`public/maps/${map}/props.json`));
  const manifest = JSON.parse(readFileSync(`public/maps/${map}/map.json`));
  for (const p of props.instances.filter(p => p.weapon?.ordnance?.LaserTexture)) {
    const name = p.weapon.ordnance.LaserTexture;
    assert.ok(manifest.textures[name], `${map}: ${name} is a map dependency`);
    assert.ok(existsSync(`public/maps/${map}/${manifest.textures[name]}`));
  }
});
