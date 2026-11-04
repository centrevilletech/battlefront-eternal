import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createPagePrediction } from '../src/runtime/page-prediction.js';
import { InfantryPrediction } from '../src/runtime/infantry-prediction.js';
import { WeaponPrediction } from '../src/runtime/weapon-prediction.js';
import { NetworkSession } from '../src/runtime/network-session.js';
import { InputLead, replayTicks } from '../src/runtime/reconcile.js';
import { PlayoutClock } from '../src/runtime/playout.js';
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
    for (const name of ['actors', 'weapons', 'vehicles', 'remotes', 'corrections', 'reconcile', 'replay', 'playout']) assert.equal(owner[name].bridge, owner.bridge);
    assert.equal(owner.bridge.x, rays.x);
    assert.equal(owner.playout.handle, 0);
    assert.equal(owner.playout.calls, 0);
    const playout = new PlayoutClock({}, { native: owner.playout });
    playout.arrive(3, 1 / 60);
    assert.equal(playout.advance(0), 1);
    assert.ok(owner.playout.handle, 'the page clock allocates its playout owner on first use');
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
    const lead = new InputLead({}, { native: owner.reconcile });
    lead.arrive(1, 1 / 60);
    assert.equal(lead.estimator.estimate(1 / 60), 1);
    assert.ok(owner.reconcile.handle);
    assert.equal(replayTicks([{ tick: 1, generation: 0, sequence: 1, actions: [], down: [], look: [] }], 0, 0, [], () => {}, {}, { native: owner.replay }), 1);
    assert.ok(owner.replay.calls);
    assert.equal(owner.replay.activeSessions, 0);
    instance.exports.memory.grow(1);
    playout.arrive(4, 2 / 60);
    assert.ok(playout.advance(1 / 60) > 1);
    lifecycle.dispatchEvent(new Event('pagehide'));
    assert.equal(owner.disposed, true);
    assert.equal(cached.handle, 0);
    assert.equal(weaponSession.handle, 0);
    assert.equal(owner.corrections.handle, 0);
    assert.equal(owner.reconcile.handle, 0);
    assert.equal(owner.replay.activeSessions, 0);
    assert.equal(owner.playout.handle, 0);
    assert.equal(unit.combat.arsenal, authority);
    assert.throws(() => owner.actors.bind(actor.soldier), /disposed/);
    assert.throws(() => owner.weapons.createArsenal(authority), /disposed/);
    assert.throws(() => owner.vehicles.create({}), /disposed/);
    assert.throws(() => owner.remotes.create({}, {}), /disposed/);
    assert.throws(() => owner.corrections.decodeWeapon(bytes), /disposed/);
    assert.throws(() => lead.arrive(2, 2 / 60), /disposed/);
    assert.throws(() => replayTicks([], 0, 0, [], () => {}, {}, { native: owner.replay }), /disposed/);
    assert.throws(() => playout.advance(0), /disposed/);
    session.dispose(); owner.dispose();
  } finally { session.dispose(); owner.dispose(); course.dispose(); }
});

test('page owner attempts every release and preserves the first cleanup failure through reentry', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  const owner = createPagePrediction(instance, course.world), released = [];
  try {
    const bytes = captureWeaponState(createWeaponUnit([weaponDefinition('cleanup')]).combat.arsenal.weapons[0]);
    owner.corrections.decodeWeapon(bytes);
    new PlayoutClock({}, { native: owner.playout });
    assert.ok(owner.playout.handle);
    for (const name of ['actors', 'weapons', 'vehicles', 'remotes', 'corrections', 'reconcile', 'replay', 'playout']) {
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
    assert.deepEqual(released, ['actors', 'weapons', 'vehicles', 'remotes', 'corrections', 'reconcile', 'replay', 'playout']);
    assert.equal(owner.corrections.handle, 0);
    assert.equal(owner.playout.handle, 0);
    owner.dispose(); assert.equal(released.length, 8);
  } finally { owner.dispose(); course.dispose(); }
});

test('an unused page playout owner closes without allocating native state', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  const owner = createPagePrediction(instance, course.world);
  try {
    assert.equal(owner.playout.handle, 0);
    assert.equal(owner.playout.calls, 0);
    owner.dispose(); owner.dispose();
    assert.equal(owner.playout.handle, 0);
    assert.equal(owner.playout.calls, 0);
    assert.throws(() => new PlayoutClock({}, { native: owner.playout }), /disposed/);
    assert.equal(owner.playout.calls, 0);
  } finally { owner.dispose(); course.dispose(); }
});

test('page close inside playout preserves the current source continuation and callback failure', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  function execute(owner, failure) {
    const clock = new PlayoutClock({ window: 3 }, { native: owner?.playout }), events = [];
    clock.arrive(9, .15);
    const ring = clock.gaps, measure = clock.measure;
    let callsAtClose, value, threw = false, error;
    const session = new NetworkSession({ lifecycle: null, transport: { dispose() { events.push('transport'); } }, onDispose() {
      events.push('owner'); owner?.dispose(); session.dispose();
      callsAtClose = owner?.playout.calls;
    } });
    clock.measure = function () {
      events.push('measure');
      const delay = measure.call(this);
      session.dispose(); events.push('closed');
      if (failure) throw failure.value;
      return delay;
    };
    try { value = clock.advance(.01); } catch (caught) { threw = true; error = caught; }
    finally { session.dispose(); }
    const { measure: ignored, ...state } = clock;
    return { clock, callsAtClose, result: { value, threw, error, events, state, sameRing: clock.gaps === ring } };
  }
  try {
    for (const failure of [null, { value: undefined }, { value: false }, { value: Error('measure failure') }]) {
      const owner = createPagePrediction(instance, course.world);
      try {
        const expected = execute(null, failure), actual = execute(owner, failure);
        assert.deepEqual(actual.result, expected.result);
        assert.equal(actual.result.threw, Boolean(failure));
        if (failure) assert.equal(actual.result.error, failure.value);
        assert.deepEqual(actual.result.events, ['measure', 'transport', 'owner', 'closed']);
        assert.equal(owner.disposed, true);
        assert.equal(owner.playout.handle, 0);
        assert.ok(actual.callsAtClose > 0);
        assert.equal(owner.playout.calls, actual.callsAtClose);
        assert.throws(() => actual.clock.reset(), /disposed/);
        assert.equal(owner.playout.calls, actual.callsAtClose);
      } finally { owner.dispose(); }
    }
  } finally { course.dispose(); }
});

test('page close inside nested replay finishes both cursors and preserves callback failure', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays);
  const commands = [1, 2].map(tick => ({ tick, generation: 0, sequence: tick, actions: [], down: [], look: [] }));
  try {
    for (const failure of [false, true]) {
      const owner = createPagePrediction(instance, course.world), events = [];
      const options = { native: owner.replay };
      let caught = false;
      try {
        const last = replayTicks(commands, 0, 0, [], tick => {
          events.push(['outer', tick]);
          if (tick !== 1) return;
          replayTicks(commands, 0, 0, [], inner => {
            events.push(['inner', inner]);
            if (inner !== 1) return;
            assert.equal(owner.replay.activeSessions, 2);
            owner.dispose();
            assert.equal(owner.replay.activeSessions, 2);
            if (failure) throw undefined;
          }, {}, options);
          assert.equal(owner.replay.activeSessions, 1);
        }, {}, options);
        assert.equal(last, 2);
      } catch (error) { caught = true; assert.equal(error, undefined); }
      assert.equal(caught, failure);
      assert.deepEqual(events, failure ? [['outer', 1], ['inner', 1]] : [['outer', 1], ['inner', 1], ['inner', 2], ['outer', 2]]);
      assert.equal(owner.replay.activeSessions, 0);
      assert.equal(owner.disposed, true);
      assert.throws(() => replayTicks(commands, 0, 0, [], () => {}, {}, options), /disposed/);
      owner.dispose();
    }
  } finally { course.dispose(); }
});
