// Service droids and the repair tool as the player sees them: a droid's pose
// derives from its tick's plain state alone (a page showing a worker's
// battle gets only that), its serve pulse marks who it served, and a repair
// lock travels as a value (the sparks are the page's own: arc-caster.test.js).
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { PowerupStation, LiveObjects } from '../src/props.js';
import { repairLockKey, repairLockTarget } from '../src/combat.js';

const IDENTITY = new THREE.Matrix4().toArray();

// A gonk's rig: one bone to rock, no arms.
function rig(names) {
  const root = new THREE.Group();
  const byName = {};
  const bind = new Map();
  for (const name of names) {
    const bone = new THREE.Object3D();
    bone.name = name;
    root.add(bone);
    byName[name] = bone;
    bind.set(bone, { q: bone.quaternion.clone(), p: bone.position.clone() });
  }
  return { root, byName, bind, model: null };
}

function station(settings = {}) {
  const desc = {
    base: 'powerupstation', radius: 5, matrix: IDENTITY, team: 0,
    settings: { MaxHealth: '80', PowerupDelay: '1.0', SoldierHealth: '15.0', SoldierAmmo: '1.0', IdleRotateSpeed: '0.5', IdleWaitTime: '3.0',
      IdleWobbleNode: 'gonk1', IdleWobbleFactor: '0.08', ActiveRotateNode: ['arm_a', 'arm_b'], ActiveSpinNode: 'spin', ...settings },
  };
  return new PowerupStation(desc, { built: rig(['gonk1', 'arm_a', 'arm_b', 'spin']), destroyed: null, unbuilt: null }, {}, null);
}

// What the replica carries of a building: its own plain fields.
function plain(object) {
  const out = {};
  for (const key of Object.keys(object)) {
    const value = object[key];
    if (value === null || ['number', 'string', 'boolean'].includes(typeof value)) out[key] = value;
  }
  return out;
}

function soldier(x, { player = false, health = 100, reserve = 0 } = {}) {
  return { player, position: new THREE.Vector3(x, 0, 0), health, maxHealth: 300,
    weapons: [{ def: { ammo: 5 }, clip: 1, perShot: 1, reserve, bottomless: false }] };
}

test('a serve pulse says who it served and what it gave, and tells the hook', () => {
  const droid = station();
  const given = [];
  droid.onServe = (target, gave) => given.push([target, gave]);
  const me = soldier(2, { player: true, health: 100, reserve: 1 });
  const bot = soldier(3, { health: 300, reserve: 0 });
  const far = soldier(9, { player: true });
  droid.update(1, [me, bot, far]);
  assert.equal(droid.mode, 'active');
  assert.equal(droid.servedPlayer, true);
  assert.equal(droid.servedHealth, true);
  assert.equal(droid.servedAmmo, true);
  assert.deepEqual(given, [[me, 3], [bot, 2]]);
  assert.equal(me.health, 115);
  assert.equal(far.health, 100);
  assert.ok(droid.supplying(me.position));
  assert.ok(!droid.supplying(far.position));
  // Nobody left to serve: the next pulse ends the service.
  droid.update(1, [far]);
  assert.equal(droid.mode, 'waiting');
  assert.equal(droid.servedPlayer, false);
  assert.ok(!droid.supplying(me.position));
});

test('a page poses a droid from the replicated plain state alone; the idle cycle is the page\'s and never travels', () => {
  const world = station();
  const page = station();
  const me = soldier(1, { player: true, health: 10 });
  const rigOf = (droid) => {
    const gonk = droid.rig.byName.gonk1, arm = droid.rig.byName.arm_b, spin = droid.rig.byName.spin;
    return [droid.turner.rotation.y, ...gonk.quaternion.toArray(), gonk.position.y, ...arm.quaternion.toArray(), ...spin.quaternion.toArray()];
  };
  const untouched = rigOf(world);
  let t = 0, turned = false, served = false;
  for (let i = 0; i < 600; i++) {
    t += 1 / 60;
    world.update(1 / 60, t > 6 ? [me] : []);
    assert.deepEqual(rigOf(world), untouched, `tick ${i}: the tick poses nothing`);
    const fields = plain(world);
    for (const key of ['t', 'direction', 'rate', 'yaw']) assert.ok(!(key in fields), `${key} never travels`);
    Object.assign(page, fields);
    page.pose(1 / 60);
    const once = rigOf(page);
    page.pose(0); // idempotent: posing again as it is is the same pose
    assert.deepEqual(rigOf(page), once, `tick ${i}, mode ${world.mode}`);
    if (world.mode === 'waiting' && page.turner.rotation.y !== 0) turned = true;
    if (world.mode === 'active' && page.rig.byName.gonk1.position.y !== 0) served = true;
  }
  assert.equal(world.mode, 'active');
  assert.ok(world.active > 0, 'the serving clock travels');
  assert.ok(turned, 'idle, the page turned the droid itself');
  assert.ok(served, 'serving, the page dipped the gonk on the replicated clock');
  assert.ok(page.supplying(me.position), 'and the HUD\'s supply indicator reads the replicated mode');
});

test('a repair lock travels as a key and comes back as the target', () => {
  const buildings = [{}, {}, {}];
  const vehicleTargets = [{}, {}];
  const live = { buildings, vehicleTargets };
  assert.equal(repairLockKey(live, buildings[2]), 'l2');
  assert.equal(repairLockKey(live, vehicleTargets[1]), 'v1');
  assert.equal(repairLockKey(live, {}), '');
  assert.equal(repairLockKey(live, null), '');
  assert.equal(repairLockTarget(live, 'l2'), buildings[2]);
  assert.equal(repairLockTarget(live, 'v1'), vehicleTargets[1]);
  assert.equal(repairLockTarget(live, 'l9'), null);
  assert.equal(repairLockTarget(live, ''), null);
  assert.equal(repairLockTarget(null, 'l0'), null);
});

test('LiveObjects.present poses living stations and names the serving one', () => {
  const live = Object.create(LiveObjects.prototype);
  const a = station(), b = station();
  Object.assign(live, { stations: [a, b], holograms: [], time: 0 });
  const me = soldier(1, { player: true, health: 10 });
  b.update(1, [me]);
  b.idle.yaw = 1.25;
  a.state = 'destroyed';
  a.idle.yaw = 2;
  live.present(1 / 60, null);
  assert.equal(b.turner.rotation.y, 1.25);
  assert.equal(a.turner.rotation.y, 0);
  assert.equal(live.supplying(me.position), b);
  assert.equal(live.supplying(new THREE.Vector3(20, 0, 0)), null);
});
