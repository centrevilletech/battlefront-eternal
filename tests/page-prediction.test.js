import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createPagePrediction } from '../src/runtime/page-prediction.js';
import { InfantryPrediction } from '../src/runtime/infantry-prediction.js';
import { WeaponPrediction } from '../src/runtime/weapon-prediction.js';
import { NetworkSession } from '../src/runtime/network-session.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
import { actorCourse, createPredictionActor, actorInput } from './fixtures/prediction-actor-cases.js';
import { createWeaponUnit, weaponDefinition } from './fixtures/prediction-weapon-cases.js';

test('page teardown releases cached native actors, active weapons and correction readers on the shared instance', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  const owner = createPagePrediction(instance, course.world);
  const scenario = { kind: 'soldier', pattern: 'walk' }, actor = createPredictionActor(scenario, course);
  const infantry = new InfantryPrediction(actor.soldier, actor.combat, actor.combat.kick, { native: owner.actors });
  const unit = createWeaponUnit([weaponDefinition('page')]), authority = unit.combat.arsenal;
  const weapons = new WeaponPrediction({ native: owner.weapons });
  const lifecycle = new EventTarget();
  const session = new NetworkSession({ lifecycle, transport: { dispose() {} }, onDispose() {
    infantry.dispose(); weapons.dispose(); owner.dispose();
  } });
  try {
    for (const name of ['actors', 'weapons', 'vehicles', 'remotes', 'corrections']) assert.equal(owner[name].bridge, owner.bridge);
    assert.equal(owner.bridge.x, rays.x);
    infantry.step(actorInput(scenario, 0), 1 / 60);
    const cached = owner.actors.bind(actor.soldier);
    assert.ok(cached.handle, 'an actor owner survives its short-lived replay lease');
    weapons.begin(unit, { tick: 0 });
    weapons.step({ actions: ['firePrimary'], down: ['Mouse0'] }, 1, () => {}, [1, null]);
    const weaponSession = weapons.nativeSession;
    assert.ok(weaponSession.handle);
    const bytes = captureWeaponState(authority.weapons[0]);
    owner.corrections.decodeWeapon(bytes);
    assert.ok(owner.corrections.handle);
    instance.exports.memory.grow(1);
    lifecycle.dispatchEvent(new Event('pagehide'));
    assert.equal(owner.disposed, true);
    assert.equal(cached.handle, 0);
    assert.equal(weaponSession.handle, 0);
    assert.equal(owner.corrections.handle, 0);
    assert.equal(unit.combat.arsenal, authority);
    assert.throws(() => owner.actors.bind(actor.soldier), /disposed/);
    assert.throws(() => owner.weapons.createArsenal(authority), /disposed/);
    assert.throws(() => owner.vehicles.create({}), /disposed/);
    assert.throws(() => owner.remotes.create({}, {}), /disposed/);
    assert.throws(() => owner.corrections.decodeWeapon(bytes), /disposed/);
    session.dispose(); owner.dispose();
  } finally { session.dispose(); owner.dispose(); course.dispose(); }
});

test('page owner attempts every release and preserves the first cleanup failure through reentry', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  const owner = createPagePrediction(instance, course.world), released = [];
  try {
    const bytes = captureWeaponState(createWeaponUnit([weaponDefinition('cleanup')]).combat.arsenal.weapons[0]);
    owner.corrections.decodeWeapon(bytes);
    for (const name of ['actors', 'weapons', 'vehicles', 'remotes', 'corrections']) {
      const dispose = owner[name].dispose;
      owner[name].dispose = () => {
        dispose(); released.push(name); owner.dispose();
        if (name === 'actors') throw undefined;
        if (name === 'vehicles') throw Error('later failure');
      };
    }
    let caught = false;
    try { owner.dispose(); } catch (error) { caught = true; assert.equal(error, undefined); }
    assert.equal(caught, true);
    assert.deepEqual(released, ['actors', 'weapons', 'vehicles', 'remotes', 'corrections']);
    assert.equal(owner.corrections.handle, 0);
    owner.dispose(); assert.equal(released.length, 5);
  } finally { owner.dispose(); course.dispose(); }
});
